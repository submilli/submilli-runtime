//! The packages a playground serves (KTD21, R5–R7): the blueprint's package closure as
//! the ready record and `status` report it, and the freshness check every run gets,
//! whoever sends it.
//!
//! Before a run that imports a package is compiled, the server calls the playground's
//! [`Freshness`] hook. It compares each project package the run reaches with the copy
//! installed in the package store, by the source the artifact embeds, and rebuilds and
//! reinstalls the ones that changed on disk, one check at a time and off the async
//! workers. A package that no longer builds fails the run as a package-resolution
//! error naming it and carrying the build diagnostic, so the stale copy never runs. A
//! watcher on the package store evicts the server's prepared packages after an
//! install from outside the playground, such as `submilli build publish-local`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use notify_debouncer_mini::notify::RecursiveMode;
use serde::Serialize;
use submilli_blueprint::Blueprint;
use submilli_build::{
    DependencyKind, DriverError, PackageName, PackageStore, PackageStoreError, ProjectManifest,
    build_packages, install_packages, package_sources, parse_manifest, read_installed_sources,
};
use submilli_server::{AppState, PreExecute, PreExecuteHook, PreExecuteRefusal};

/// How long the package store must be quiet before the playground evicts: an install
/// writes several files per package.
const STORE_DEBOUNCE: Duration = Duration::from_millis(200);

/// One package of the blueprint's closure, as the ready record and `status` list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ClosureEntry {
    pub(crate) name: String,
    pub(crate) version: String,
    /// `blueprint` when the blueprint's `packages:` lists it, `dependency` when it
    /// arrived only as another package's dependency.
    pub(crate) origin: Origin,
    /// Whether a program may import it: only packages the blueprint lists.
    pub(crate) importable: bool,
    /// Whether the project's `submilli.toml` builds it, so the playground rebuilds it
    /// when its source changes.
    pub(crate) project: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    Blueprint,
    Dependency,
}

/// Why a package could not be made ready for a run. Each kind keeps what went wrong
/// and says what fixes it; none is a policy denial.
#[derive(Debug)]
pub(crate) enum ResolutionFailure {
    /// The project's `submilli.toml` does not parse, so its packages cannot be built.
    Manifest { path: PathBuf, message: String },
    /// A project package changed on disk and no longer builds.
    Build {
        package: String,
        diagnostic: String,
        manifest_dir: PathBuf,
    },
    /// The store does not hold a package, or holds one that cannot be loaded.
    Store(PackageStoreError),
    /// The rebuilt package could not be written to the store.
    Install { package: String, message: String },
    /// The check's blocking task ended before it finished.
    Stopped(String),
}

impl fmt::Display for ResolutionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest { path, message } => write!(
                f,
                "{} does not parse, so the project's packages cannot be built: {message}; fix \
                 it and run again",
                path.display()
            ),
            Self::Build {
                package,
                diagnostic,
                manifest_dir,
            } => write!(
                f,
                "package `{package}` changed on disk and no longer builds, so the run did not \
                 start and the installed copy was not used. Fix the error and run again; \
                 `submilli build check` in {} shows it too.\n{}",
                manifest_dir.display(),
                diagnostic.trim_end()
            ),
            // The store's own message names the package and the command that fixes it.
            Self::Store(error) => error.fmt(f),
            Self::Install { package, message } => write!(
                f,
                "package `{package}` was rebuilt but could not be installed: {message}; run \
                 `submilli build publish-local` to retry"
            ),
            Self::Stopped(error) => {
                write!(f, "the package check stopped before it finished: {error}")
            }
        }
    }
}

impl std::error::Error for ResolutionFailure {}

/// What one check did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Synced {
    /// Project packages rebuilt and reinstalled because their source changed.
    pub(crate) reinstalled: Vec<String>,
    /// Whether the server's prepared packages must be evicted: something it may have
    /// cached was replaced, by this check or by an install from outside.
    pub(crate) evict: bool,
}

/// The project's packages and the store they install into.
pub(crate) struct ProjectPackages {
    manifest_path: PathBuf,
    manifest_dir: PathBuf,
    store_root: PathBuf,
    /// Poison means a panic interrupted a check; AGENTS.md permits the poisoned-lock
    /// panic, since the stamps may be half-updated.
    seen: Mutex<Seen>,
}

#[derive(Default)]
struct Seen {
    /// When each project package's installed copy was last written, as the last check
    /// saw it: a change means an install from outside the playground.
    stamps: BTreeMap<String, Option<SystemTime>>,
    /// The last build that failed, by the sources it was built from, so a run against
    /// the same broken source reports it again without rebuilding.
    failed: Option<(u64, String, String)>,
}

impl ProjectPackages {
    /// `manifest_dir` holds the project's `submilli.toml`.
    pub(crate) fn new(manifest_dir: &Path, store_root: PathBuf) -> Self {
        Self {
            manifest_path: manifest_dir.join("submilli.toml"),
            manifest_dir: manifest_dir.to_path_buf(),
            store_root,
            seen: Mutex::default(),
        }
    }

    pub(crate) fn store_root(&self) -> &Path {
        &self.store_root
    }

    fn store(&self) -> PackageStore {
        PackageStore::new(&self.store_root)
    }

    fn manifest(&self) -> Result<ProjectManifest, ResolutionFailure> {
        let manifest_failure = |message: String| ResolutionFailure::Manifest {
            path: self.manifest_path.clone(),
            message,
        };
        let text = std::fs::read_to_string(&self.manifest_path)
            .map_err(|error| manifest_failure(error.to_string()))?;
        parse_manifest(&text, &self.manifest_dir).map_err(|diagnostics| {
            manifest_failure(
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })
    }

    /// Bring the project packages `wanted` reaches up to date with their source,
    /// rebuilding and reinstalling the ones that changed. Blocks on the filesystem and
    /// the compiler, so an async caller runs it on a blocking thread.
    pub(crate) fn sync(&self, wanted: &BTreeSet<String>) -> Result<Synced, ResolutionFailure> {
        if wanted.is_empty() {
            return Ok(Synced::default());
        }
        let manifest = self.manifest()?;
        let reached = project_closure(&manifest, wanted);
        if reached.is_empty() {
            return Ok(Synced::default());
        }
        let store = self.store();
        let mut seen = self
            .seen
            .lock()
            .expect("the package check's state is poisoned by an earlier panic");
        let mut evict = false;
        let mut stale = Vec::new();
        let mut fingerprint = Fingerprint::default();
        for package in manifest
            .packages
            .iter()
            .filter(|package| reached.contains(package.name.as_str()))
        {
            let name = package.name.as_str();
            let (sources, documentation) = match package_sources(&self.manifest_dir, package) {
                Ok(read) => read,
                Err(error) => {
                    return Err(ResolutionFailure::Build {
                        package: name.to_owned(),
                        diagnostic: error.to_string(),
                        manifest_dir: self.manifest_dir.clone(),
                    });
                }
            };
            let located = store.locate(name).map_err(ResolutionFailure::Store)?;
            let stamp = located
                .as_ref()
                .and_then(|located| installed_at(&located.dir));
            if let Some(previous) = seen.stamps.insert(name.to_owned(), stamp)
                && previous != stamp
            {
                evict = true;
            }
            let fresh = located
                .and_then(|located| read_installed_sources(&located.dir).ok())
                .is_some_and(|installed| {
                    let mut embedded = installed.sources;
                    embedded.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
                    let mut on_disk = sources.clone();
                    on_disk.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
                    embedded == on_disk
                        && installed.documentation == documentation
                        && installed.metadata.package_version == package.version.as_str()
                        && installed.metadata.description == package.description
                });
            if !fresh {
                fingerprint.add(name, &sources, &documentation);
                stale.push(package.name.clone());
            }
        }
        if stale.is_empty() {
            seen.failed = None;
            return Ok(Synced {
                reinstalled: Vec::new(),
                evict,
            });
        }
        let fingerprint = fingerprint.finish();
        if let Some((failed, package, diagnostic)) = &seen.failed
            && *failed == fingerprint
        {
            return Err(ResolutionFailure::Build {
                package: package.clone(),
                diagnostic: diagnostic.clone(),
                manifest_dir: self.manifest_dir.clone(),
            });
        }
        let reinstalled = match self.rebuild(&manifest, &store, &stale) {
            Ok(reinstalled) => reinstalled,
            Err(failure) => {
                if let ResolutionFailure::Build {
                    package,
                    diagnostic,
                    ..
                } = &failure
                {
                    seen.failed = Some((fingerprint, package.clone(), diagnostic.clone()));
                }
                return Err(failure);
            }
        };
        seen.failed = None;
        for name in &reinstalled {
            let stamp = store
                .locate(name)
                .ok()
                .flatten()
                .and_then(|located| installed_at(&located.dir));
            seen.stamps.insert(name.clone(), stamp);
        }
        Ok(Synced {
            reinstalled,
            evict: true,
        })
    }

    /// Build each stale package with the siblings it needs, and install the stale ones.
    fn rebuild(
        &self,
        manifest: &ProjectManifest,
        store: &PackageStore,
        stale: &[PackageName],
    ) -> Result<Vec<String>, ResolutionFailure> {
        let mut installed = Vec::new();
        for name in stale {
            if installed.iter().any(|done: &String| done == name.as_str()) {
                continue;
            }
            let built = build_packages(manifest, &self.manifest_dir, store, Some(name))
                .map_err(|error| self.build_failure(name, error))?;
            let wanted: Vec<_> = built
                .into_iter()
                .filter(|package| stale.contains(&package.name))
                .filter(|package| !installed.iter().any(|done| done == package.name.as_str()))
                .collect();
            install_packages(store, &wanted).map_err(|error| ResolutionFailure::Install {
                package: name.as_str().to_owned(),
                message: error.to_string(),
            })?;
            installed.extend(
                wanted
                    .iter()
                    .map(|package| package.name.as_str().to_owned()),
            );
        }
        Ok(installed)
    }

    fn build_failure(&self, requested: &PackageName, error: DriverError) -> ResolutionFailure {
        let (package, diagnostic) = match error {
            DriverError::Compile { package, rendered } => (package.as_str().to_owned(), rendered),
            other => (requested.as_str().to_owned(), other.to_string()),
        };
        ResolutionFailure::Build {
            package,
            diagnostic,
            manifest_dir: self.manifest_dir.clone(),
        }
    }

    /// The blueprint's package closure as the store holds it now.
    pub(crate) fn closure(
        &self,
        blueprint: &Blueprint,
    ) -> Result<Vec<ClosureEntry>, ResolutionFailure> {
        let project: BTreeSet<String> = self
            .manifest()
            .map(|manifest| {
                manifest
                    .packages
                    .iter()
                    .map(|package| package.name.as_str().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let artifacts = self
            .store()
            .load_closure(blueprint.packages.iter().map(String::as_str))
            .map_err(ResolutionFailure::Store)?;
        let mut entries: Vec<ClosureEntry> = artifacts
            .into_iter()
            .map(|artifact| {
                let name = artifact.metadata.package_name;
                let listed = blueprint.packages.contains(&name);
                ClosureEntry {
                    version: artifact.metadata.package_version,
                    origin: if listed {
                        Origin::Blueprint
                    } else {
                        Origin::Dependency
                    },
                    importable: listed,
                    project: project.contains(&name),
                    name,
                }
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// At start: bring the blueprint's project packages up to date, then read its
    /// closure. A package that cannot be built or found fails the start.
    pub(crate) fn prepare(
        &self,
        blueprint: &Blueprint,
    ) -> Result<(Synced, Vec<ClosureEntry>), ResolutionFailure> {
        let synced = self.sync(&blueprint.packages)?;
        let closure = self.closure(blueprint)?;
        Ok((synced, closure))
    }
}

/// The project packages `wanted` names, with the sibling packages they depend on.
fn project_closure(manifest: &ProjectManifest, wanted: &BTreeSet<String>) -> BTreeSet<String> {
    let mut reached = BTreeSet::new();
    let mut queue: Vec<String> = wanted.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let Some(package) = manifest
            .packages
            .iter()
            .find(|package| package.name.as_str() == name)
        else {
            continue;
        };
        if !reached.insert(name) {
            continue;
        }
        queue.extend(
            package
                .dependencies
                .iter()
                .filter(|dependency| dependency.kind == DependencyKind::Sibling)
                .map(|dependency| dependency.name.as_str().to_owned()),
        );
    }
    reached
}

/// When an installed package was last written: an install rewrites its metadata.
fn installed_at(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir.join("metadata.json"))
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// A digest of the sources a failed build read.
#[derive(Default)]
struct Fingerprint(sha2::Sha256);

impl Fingerprint {
    fn add(&mut self, name: &str, sources: &[submilli_build::ArtifactSource], docs: &str) {
        use sha2::Digest;
        for part in [name, docs] {
            self.0.update((part.len() as u64).to_le_bytes());
            self.0.update(part.as_bytes());
        }
        for source in sources {
            for part in [source.path.as_str(), source.text.as_str()] {
                self.0.update((part.len() as u64).to_le_bytes());
                self.0.update(part.as_bytes());
            }
        }
    }

    fn finish(self) -> u64 {
        use sha2::Digest;
        let digest = self.0.finalize();
        let mut first = [0_u8; 8];
        first.copy_from_slice(&digest[..8]);
        u64::from_le_bytes(first)
    }
}

/// The server's pre-execute hook: a run's project packages are brought up to date
/// before it compiles. The blueprint watcher brings the project packages an edit newly
/// names up to date through it too, before the edit is validated. Checks are
/// serialized, so two runs after one edit rebuild once.
pub(crate) struct Freshness {
    packages: Arc<ProjectPackages>,
    turn: tokio::sync::Mutex<()>,
}

impl Freshness {
    pub(crate) fn new(packages: Arc<ProjectPackages>) -> Self {
        Self {
            packages,
            turn: tokio::sync::Mutex::new(()),
        }
    }

    /// Before an edit is validated: build and install the project packages it needs
    /// that the store lacks or that the version in force did not list. A package both
    /// list stays the pre-execute check's to refresh, so an edit elsewhere in the
    /// blueprint is not refused while that package's source is mid-change.
    pub(crate) async fn prepare_edit(
        &self,
        state: &AppState,
        blueprint: Blueprint,
        in_force: Option<Blueprint>,
    ) -> Result<(), ResolutionFailure> {
        if blueprint.packages.is_empty() {
            return Ok(());
        }
        self.check(state, move |packages| {
            let store = packages.store();
            let wanted: BTreeSet<String> = blueprint
                .packages
                .iter()
                .filter(|name| {
                    in_force
                        .as_ref()
                        .is_none_or(|in_force| !in_force.packages.contains(*name))
                        || !matches!(store.locate(name), Ok(Some(_)))
                })
                .cloned()
                .collect();
            packages.sync(&wanted)
        })
        .await
    }

    /// Run one check on a blocking thread, one at a time, and evict the server's
    /// prepared packages when it replaced one.
    async fn check(
        &self,
        state: &AppState,
        check: impl FnOnce(&ProjectPackages) -> Result<Synced, ResolutionFailure> + Send + 'static,
    ) -> Result<(), ResolutionFailure> {
        let _turn = self.turn.lock().await;
        let packages = Arc::clone(&self.packages);
        let synced = tokio::task::spawn_blocking(move || check(&packages))
            .await
            .map_err(|error| ResolutionFailure::Stopped(error.to_string()))??;
        for name in &synced.reinstalled {
            note(&format!(
                "reinstalled {name}: its source changed since it was installed"
            ));
        }
        if synced.evict {
            state.evict_all_prepared_packages();
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl PreExecuteHook for Freshness {
    async fn before_execute(&self, run: PreExecute<'_>) -> Result<(), PreExecuteRefusal> {
        if run.packages.is_empty() {
            return Ok(());
        }
        let wanted = run.packages.clone();
        self.check(run.state, move |packages| packages.sync(&wanted))
            .await
            .map_err(|failure| PreExecuteRefusal {
                message: failure.to_string(),
            })
    }
}

/// Watches the package store and evicts the server's prepared packages after any
/// change there, so an install from outside the playground is picked up.
pub(crate) struct StoreWatch {
    _debouncer: notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>,
}

pub(crate) fn watch_store(state: AppState, root: &Path) -> Result<StoreWatch> {
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let mut debouncer = notify_debouncer_mini::new_debouncer(
        STORE_DEBOUNCE,
        move |_: notify_debouncer_mini::DebounceEventResult| {
            // An error may have dropped events; evicting costs only a reload.
            state.evict_all_prepared_packages();
        },
    )
    .context("starting the package store watcher")?;
    debouncer
        .watcher()
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", root.display()))?;
    Ok(StoreWatch {
        _debouncer: debouncer,
    })
}

/// The detached child's stderr is its log.
fn note(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "playground: {message}");
}

#[cfg(test)]
mod tests;
