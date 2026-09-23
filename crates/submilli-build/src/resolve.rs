//! Plan a project's GitHub-dependency closure.
//!
//! A `submilli.toml` may declare cross-repo dependencies as GitHub sources
//! (`{ github = "...", rev = "<sha>" }`). [`resolve_github_closure`] walks that
//! closure — fetch each dependency repo at its pinned SHA, read its own manifest,
//! recurse into *its* GitHub deps — and returns a [`GithubClosure`]: the fetched
//! repos in dependency order, ready to install. It is the network half; the
//! network-free install of the plan is [`install_plan`](crate::install::install_plan),
//! a loop over [`install_from_dir`](crate::install::install_from_dir). This
//! mirrors the single-repo `fetch` → `install_from_dir` split.
//!
//! The network fetch is injected through [`RepoFetcher`] so the closure logic
//! stays unit-testable with fixture repos; the `ureq`-backed implementation
//! lives in `submilli-shared`.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::lockfile::{LockedPackage, Lockfile};
use crate::{
    BuildDiagnostic, DependencyKind, DependencySource, GithubSource, PackageManifest, PackageName,
    PackageSource, PackageStore, ProjectManifest, load_manifest,
};

/// A repo fetched at a pinned commit, ready to install from.
pub struct FetchedRepo {
    /// GitHub org/user the repo belongs to (resolved from the source URL).
    pub org: String,
    /// GitHub repo name.
    pub repo: String,
    /// Extracted repo root (contains `submilli.toml`).
    pub root: PathBuf,
    /// sha256 of the downloaded source tarball.
    pub source_hash: String,
    /// Keeps any backing temp storage alive for as long as this value lives.
    pub keep_alive: Option<Box<dyn Any + Send>>,
}

/// A network fetch of a GitHub repo at a concrete commit SHA.
pub trait RepoFetcher {
    fn fetch(&self, url: &str, sha: &str) -> Result<FetchedRepo, FetchError>;
}

#[derive(Debug)]
pub struct FetchError {
    pub message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

#[derive(Debug)]
pub enum ResolveError {
    /// Fetching a dependency repo failed.
    Fetch {
        name: PackageName,
        url: String,
        sha: String,
        source: FetchError,
    },
    /// A dependency repo's `submilli.toml` failed to parse.
    Manifest {
        name: PackageName,
        manifest_path: PathBuf,
        manifest_text: String,
        diagnostics: Vec<BuildDiagnostic>,
    },
    /// The dependency repo doesn't declare the package the dependant asked for.
    MissingPackageInRepo {
        name: PackageName,
        url: String,
        sha: String,
        available: Vec<PackageName>,
    },
    /// Two packages in the closure require the same dependency at different SHAs.
    ShaConflict {
        name: PackageName,
        first_requirer: PackageName,
        first_sha: String,
        second_requirer: PackageName,
        second_sha: String,
    },
    /// A dependency cycle across repos.
    Cycle { path: Vec<PackageName> },
}

/// A resolved GitHub-dependency closure: the fetched repos to install (in
/// dependency order, deps first) plus the flat closure to record in the lockfile.
pub struct GithubClosure {
    /// Fetched dependency repos to install, dependency order. Empty when a
    /// satisfied lockfile means nothing needs (re)fetching.
    pub plan: Vec<PlannedInstall>,
    /// The full closure, for `submilli.lock` (dependency order).
    pub locked: Vec<LockedPackage>,
}

/// A fetched dependency repo held ready for a network-free install into the
/// store. Owns the extracted source tree until it is dropped.
pub struct PlannedInstall {
    pub name: PackageName,
    pub source: PackageSource,
    fetched: FetchedRepo,
}

impl PlannedInstall {
    /// The extracted repo root to install the package from.
    pub fn repo_dir(&self) -> &Path {
        &self.fetched.root
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Fetch {
                name,
                url,
                sha,
                source,
            } => write!(
                f,
                "fetch dependency {} from {url}@{}: {source}",
                name.as_str(),
                short(sha)
            ),
            ResolveError::Manifest {
                name,
                manifest_path,
                ..
            } => write!(
                f,
                "parse submilli.toml of dependency {} ({})",
                name.as_str(),
                manifest_path.display()
            ),
            ResolveError::MissingPackageInRepo {
                name,
                url,
                sha,
                available,
            } => write!(
                f,
                "{url}@{} does not declare package {}; it declares: {}",
                short(sha),
                name.as_str(),
                available
                    .iter()
                    .map(PackageName::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ResolveError::ShaConflict {
                name,
                first_requirer,
                first_sha,
                second_requirer,
                second_sha,
            } => write!(
                f,
                "conflicting versions of {}: {} requires {}, {} requires {}; pin both to the same commit",
                name.as_str(),
                first_requirer.as_str(),
                short(first_sha),
                second_requirer.as_str(),
                short(second_sha),
            ),
            ResolveError::Cycle { path } => write!(
                f,
                "dependency cycle: {}",
                path.iter()
                    .map(PackageName::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Plan the GitHub-dependency closure of `manifest` (the network phase): fetch
/// each dep at its pinned SHA, recurse through their manifests, and return the
/// closure in dependency order. Touches the store only to *read* it — when
/// `existing_lock` already pins every declared GitHub dep and the store holds
/// each locked package at its recorded source hash, nothing is fetched and the
/// plan is empty. Install the returned plan with
/// [`install_plan`](crate::install::install_plan).
pub fn resolve_github_closure(
    store: &PackageStore,
    manifest: &ProjectManifest,
    fetcher: &dyn RepoFetcher,
    existing_lock: Option<&Lockfile>,
) -> Result<GithubClosure, ResolveError> {
    let seeds = github_seeds(manifest);
    if seeds.is_empty() {
        return Ok(GithubClosure {
            plan: Vec::new(),
            locked: Vec::new(),
        });
    }

    if let Some(lock) = existing_lock
        && let Some(locked) = lock_satisfies(store, manifest, lock)
    {
        return Ok(GithubClosure {
            plan: Vec::new(),
            locked,
        });
    }

    let mut resolver = Resolver {
        fetcher,
        visited: BTreeMap::new(),
        in_progress: Vec::new(),
        plan: Vec::new(),
        locked: Vec::new(),
    };
    for seed in seeds {
        resolver.visit(seed.name, seed.url, seed.sha, seed.requirer)?;
    }
    Ok(GithubClosure {
        plan: resolver.plan,
        locked: resolver.locked,
    })
}

struct Seed {
    name: PackageName,
    url: String,
    sha: String,
    requirer: PackageName,
}

struct VisitedNode {
    sha: String,
    requirer: PackageName,
}

struct Resolver<'a> {
    fetcher: &'a dyn RepoFetcher,
    visited: BTreeMap<PackageName, VisitedNode>,
    in_progress: Vec<PackageName>,
    plan: Vec<PlannedInstall>,
    locked: Vec<LockedPackage>,
}

impl Resolver<'_> {
    fn visit(
        &mut self,
        name: PackageName,
        url: String,
        sha: String,
        requirer: PackageName,
    ) -> Result<(), ResolveError> {
        if let Some(node) = self.visited.get(&name) {
            if node.sha != sha {
                return Err(ResolveError::ShaConflict {
                    name: name.clone(),
                    first_requirer: node.requirer.clone(),
                    first_sha: node.sha.clone(),
                    second_requirer: requirer,
                    second_sha: sha,
                });
            }
            return Ok(());
        }
        if self.in_progress.contains(&name) {
            let mut path = self.in_progress.clone();
            path.push(name);
            return Err(ResolveError::Cycle { path });
        }

        self.in_progress.push(name.clone());

        let fetched = self
            .fetcher
            .fetch(&url, &sha)
            .map_err(|source| ResolveError::Fetch {
                name: name.clone(),
                url: url.clone(),
                sha: sha.clone(),
                source,
            })?;
        let manifest_path = fetched.root.join("submilli.toml");
        let dep_manifest =
            load_manifest(&manifest_path).map_err(|diagnostics| ResolveError::Manifest {
                name: name.clone(),
                manifest_path: manifest_path.clone(),
                manifest_text: std::fs::read_to_string(&manifest_path).unwrap_or_default(),
                diagnostics,
            })?;

        let Some(target) = dep_manifest.packages.iter().find(|p| p.name == name) else {
            return Err(ResolveError::MissingPackageInRepo {
                name,
                url,
                sha,
                available: dep_manifest
                    .packages
                    .iter()
                    .map(|p| p.name.clone())
                    .collect(),
            });
        };
        let version = target.version.as_str().to_string();

        for (dep_name, dep_url, dep_sha) in github_deps_for(&dep_manifest, &name) {
            self.visit(dep_name, dep_url, dep_sha, name.clone())?;
        }

        self.in_progress.pop();

        let source = PackageSource::Github(GithubSource {
            org: fetched.org.clone(),
            repo: fetched.repo.clone(),
            sha: sha.clone(),
            source_hash: Some(fetched.source_hash.clone()),
        });
        self.locked.push(LockedPackage {
            name: name.as_str().to_string(),
            version,
            github: url,
            sha: sha.clone(),
            source_hash: fetched.source_hash.clone(),
        });
        self.visited
            .insert(name.clone(), VisitedNode { sha, requirer });
        self.plan.push(PlannedInstall {
            name,
            source,
            fetched,
        });
        Ok(())
    }
}

/// GitHub deps declared by any package in the (root) manifest.
fn github_seeds(manifest: &ProjectManifest) -> Vec<Seed> {
    let mut seeds = Vec::new();
    for package in &manifest.packages {
        for dep in &package.dependencies {
            if dep.kind != DependencyKind::Github {
                continue;
            }
            if let Some(DependencySource::Github { url, sha }) =
                manifest.dependencies.get(&dep.name)
            {
                seeds.push(Seed {
                    name: dep.name.clone(),
                    url: url.clone(),
                    sha: sha.clone(),
                    requirer: package.name.clone(),
                });
            }
        }
    }
    seeds
}

/// GitHub deps reachable from `target` through its in-repo sibling closure.
fn github_deps_for(
    manifest: &ProjectManifest,
    target: &PackageName,
) -> Vec<(PackageName, String, String)> {
    let by_name: BTreeMap<&PackageName, &PackageManifest> =
        manifest.packages.iter().map(|p| (&p.name, p)).collect();
    let mut seen = BTreeSet::new();
    let mut queue = vec![target.clone()];
    let mut result = Vec::new();
    while let Some(pkg_name) = queue.pop() {
        if !seen.insert(pkg_name.clone()) {
            continue;
        }
        let Some(package) = by_name.get(&pkg_name) else {
            continue;
        };
        for dep in &package.dependencies {
            match dep.kind {
                DependencyKind::Sibling => queue.push(dep.name.clone()),
                DependencyKind::Github => {
                    if let Some(DependencySource::Github { url, sha }) =
                        manifest.dependencies.get(&dep.name)
                    {
                        result.push((dep.name.clone(), url.clone(), sha.clone()));
                    }
                }
                DependencyKind::External => {}
            }
        }
    }
    result
}

/// If `lock` already pins every GitHub dep the manifest declares and the store
/// holds each locked package at its recorded source hash, return the locked
/// closure so the caller can skip fetching. Otherwise `None` (re-resolve).
fn lock_satisfies(
    store: &PackageStore,
    manifest: &ProjectManifest,
    lock: &Lockfile,
) -> Option<Vec<LockedPackage>> {
    for package in &manifest.packages {
        for dep in &package.dependencies {
            if dep.kind != DependencyKind::Github {
                continue;
            }
            let DependencySource::Github { url, sha } = manifest.dependencies.get(&dep.name)?
            else {
                return None;
            };
            let entry = lock.packages.iter().find(|p| p.name == dep.name.as_str())?;
            if &entry.github != url || &entry.sha != sha {
                return None;
            }
        }
    }

    for entry in &lock.packages {
        // The lock is satisfied only by the owned root; a fallback copy would
        // leave this store depending on a directory it does not manage.
        let artifact = store.load_owned(&entry.name).ok()?;
        match artifact.metadata.source {
            Some(PackageSource::Github(gh))
                if gh.sha == entry.sha
                    && gh.source_hash.as_deref() == Some(entry.source_hash.as_str()) => {}
            _ => return None,
        }
    }

    Some(lock.packages.clone())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
