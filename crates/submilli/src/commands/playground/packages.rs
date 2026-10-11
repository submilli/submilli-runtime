//! The packages a playground serves (KTD21, R5–R7): the blueprint's package closure as
//! the ready record and `status` report it, and the freshness check every run gets,
//! whoever sends it.
//!
//! Before a run that imports a package is compiled, the server calls the playground's
//! [`Freshness`] hook. It compares each project package the run reaches with the copy
//! installed in the package store, by the source the artifact embeds, and rebuilds and
//! reinstalls the ones that changed on disk, one check at a time and off the async
//! workers. Its entry in `submilli.toml` counts as source: the installed copy must
//! carry the version, description, keywords, and dependencies it declares, and an
//! entry edited since the playground last built or checked the package rebuilds it.
//! A package that no longer builds fails the run as a package-resolution error naming
//! it and carrying the build diagnostic, so the stale copy never runs; the failure is
//! remembered against everything the build read (the manifest, the sources, and the
//! installed copies of the packages it depends on), so any change to those retries. A
//! watcher on the package store evicts the server's prepared packages after an
//! install from outside the playground, such as `submilli build publish-local`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use notify_debouncer_mini::notify::RecursiveMode;
use serde::{Deserialize, Serialize};
use submilli_blueprint::Blueprint;
use submilli_build::{
    ArtifactSource, DependencyKind, DriverError, PackageManifest, PackageName, PackageStore,
    PackageStoreError, ProjectManifest, build_packages, install_packages, package_sources,
    parse_manifest, read_installed_sources,
};
use submilli_server::{AppState, PreExecute, PreExecuteHook, PreExecuteRefusal};

use super::log::note;

/// How long the package store must be quiet before the playground evicts: an install
/// writes several files per package.
const STORE_DEBOUNCE: Duration = Duration::from_millis(200);

/// One package of the blueprint's closure, as the ready record and `status` list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The packages of the closure that depend on it, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) required_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
                "package `{package}` does not build. Fix the error and try again; `submilli \
                 build check` in {} shows it too.\n{}",
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

impl ResolutionFailure {
    /// The message a run refused before it started reports: a build failure says
    /// the installed copy was not used.
    pub(crate) fn for_run(&self) -> String {
        match self {
            Self::Build {
                package,
                diagnostic,
                manifest_dir,
            } => format!(
                "package `{package}` changed on disk and no longer builds, so the run did not \
                 start and the installed copy was not used. Fix the error and run again; \
                 `submilli build check` in {} shows it too.\n{}",
                manifest_dir.display(),
                diagnostic.trim_end()
            ),
            other => other.to_string(),
        }
    }
}

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
    /// The manifest entry each project package's installed copy was last built from or
    /// found to match. An entry edited since then means the copy is stale, whatever
    /// part of it changed.
    entries: BTreeMap<String, PackageManifest>,
    /// The last build that failed, so a run against the same broken inputs reports it
    /// again without rebuilding.
    failed: Option<FailedBuild>,
}

/// A build that failed, by a digest of everything it read.
struct FailedBuild {
    fingerprint: u64,
    package: String,
    diagnostic: String,
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
        self.read_manifest().map(|(manifest, _)| manifest)
    }

    /// The parsed manifest and the text it was parsed from.
    fn read_manifest(&self) -> Result<(ProjectManifest, String), ResolutionFailure> {
        let manifest_failure = |message: String| ResolutionFailure::Manifest {
            path: self.manifest_path.clone(),
            message,
        };
        let text = std::fs::read_to_string(&self.manifest_path)
            .map_err(|error| manifest_failure(error.to_string()))?;
        let manifest = parse_manifest(&text, &self.manifest_dir).map_err(|diagnostics| {
            manifest_failure(
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        Ok((manifest, text))
    }

    /// The sources and documentation of `package` as a build would read them now.
    fn sources_of(
        &self,
        package: &PackageManifest,
    ) -> Result<(Vec<ArtifactSource>, String), ResolutionFailure> {
        package_sources(&self.manifest_dir, package).map_err(|error| ResolutionFailure::Build {
            package: package.name.as_str().to_owned(),
            diagnostic: error.to_string(),
            manifest_dir: self.manifest_dir.clone(),
        })
    }

    /// Bring the project packages `wanted` reaches up to date with their source,
    /// rebuilding and reinstalling the ones that changed. Blocks on the filesystem and
    /// the compiler, so an async caller runs it on a blocking thread.
    pub(crate) fn sync(&self, wanted: &BTreeSet<String>) -> Result<Synced, ResolutionFailure> {
        if wanted.is_empty() {
            return Ok(Synced::default());
        }
        let (manifest, manifest_text) = self.read_manifest()?;
        let reached = project_closure(&manifest, wanted);
        if reached.is_empty() {
            return Ok(Synced::default());
        }
        let inputs = self.read_inputs(&manifest, &reached)?;
        let store = self.store();
        let mut seen = self
            .seen
            .lock()
            .expect("the package check's state is poisoned by an earlier panic");
        let (stale, evict) = seen.scan_installed(&store, &inputs)?;
        if stale.is_empty() {
            seen.failed = None;
            return Ok(Synced {
                reinstalled: Vec::new(),
                evict,
            });
        }
        let fingerprint = fingerprint(&manifest_text, &store, &inputs);
        if let Some(failed) = &seen.failed
            && failed.fingerprint == fingerprint
        {
            return Err(ResolutionFailure::Build {
                package: failed.package.clone(),
                diagnostic: failed.diagnostic.clone(),
                manifest_dir: self.manifest_dir.clone(),
            });
        }
        let reinstalled = match self.rebuild(&manifest, &store, &stale) {
            Ok(reinstalled) => reinstalled,
            Err(failure) => {
                seen.failed = match &failure {
                    // The build read the files again; remember the failure only if they
                    // are still what was fingerprinted, so it is never reported against
                    // inputs it was not computed from.
                    ResolutionFailure::Build {
                        package,
                        diagnostic,
                        ..
                    } if self.fingerprint_now(&store, &reached) == Some(fingerprint) => {
                        Some(FailedBuild {
                            fingerprint,
                            package: package.clone(),
                            diagnostic: diagnostic.clone(),
                        })
                    }
                    _ => None,
                };
                return Err(failure);
            }
        };
        seen.failed = None;
        seen.record_reinstalled(&store, &manifest, &reinstalled);
        Ok(Synced {
            reinstalled,
            evict: true,
        })
    }

    /// What a build of `reached` reads from the project: each package's entry,
    /// sources, and documentation, in manifest order.
    fn read_inputs<'a>(
        &self,
        manifest: &'a ProjectManifest,
        reached: &BTreeSet<String>,
    ) -> Result<Vec<PackageInputs<'a>>, ResolutionFailure> {
        manifest
            .packages
            .iter()
            .filter(|package| reached.contains(package.name.as_str()))
            .map(|package| {
                let (sources, documentation) = self.sources_of(package)?;
                Ok(PackageInputs {
                    package,
                    sources,
                    documentation,
                })
            })
            .collect()
    }

    /// The fingerprint [`Self::sync`] computes for `reached`, read afresh; `None` when
    /// something it reads cannot be read.
    fn fingerprint_now(&self, store: &PackageStore, reached: &BTreeSet<String>) -> Option<u64> {
        let (manifest, text) = self.read_manifest().ok()?;
        let inputs = self.read_inputs(&manifest, reached).ok()?;
        Some(fingerprint(&text, store, &inputs))
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
        let mut required_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for artifact in &artifacts {
            for dependency in &artifact.metadata.dependencies {
                required_by
                    .entry(dependency.name.clone())
                    .or_default()
                    .insert(artifact.metadata.package_name.clone());
            }
        }
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
                    required_by: required_by
                        .remove(&name)
                        .map(|names| names.into_iter().collect())
                        .unwrap_or_default(),
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

/// What a build reads of one project package.
struct PackageInputs<'a> {
    package: &'a PackageManifest,
    sources: Vec<ArtifactSource>,
    documentation: String,
}

impl Seen {
    /// Compares each package's installed copy with its inputs: the packages whose copy
    /// is stale, and whether a copy was written from outside the playground since the
    /// last check. Confirms the entry of each copy that matches.
    fn scan_installed(
        &mut self,
        store: &PackageStore,
        inputs: &[PackageInputs<'_>],
    ) -> Result<(Vec<PackageName>, bool), ResolutionFailure> {
        let mut stale = Vec::new();
        let mut evict = false;
        for input in inputs {
            let package = input.package;
            let name = package.name.as_str();
            let located = store.locate(name).map_err(ResolutionFailure::Store)?;
            let stamp = located
                .as_ref()
                .and_then(|located| installed_at(&located.dir));
            evict |= self.note_stamp(name, stamp);
            let matches = self.entries.get(name).is_none_or(|entry| entry == package)
                && located.is_some_and(|located| {
                    installed_copy_matches(
                        &located.dir,
                        package,
                        &input.sources,
                        &input.documentation,
                    )
                });
            if matches {
                self.entries.insert(name.to_owned(), package.clone());
            } else {
                stale.push(package.name.clone());
            }
        }
        Ok((stale, evict))
    }

    /// Records the copies this check installed, so the next check neither reports them
    /// as installed from outside nor finds their entries edited.
    fn record_reinstalled(
        &mut self,
        store: &PackageStore,
        manifest: &ProjectManifest,
        reinstalled: &[String],
    ) {
        for name in reinstalled {
            let stamp = store
                .locate(name)
                .ok()
                .flatten()
                .and_then(|located| installed_at(&located.dir));
            self.stamps.insert(name.clone(), stamp);
            if let Some(package) = manifest
                .packages
                .iter()
                .find(|package| package.name.as_str() == name)
            {
                self.entries.insert(name.clone(), package.clone());
            }
        }
    }

    /// Records when `name`'s installed copy was written; whether that changed since
    /// the last check, which means an install from outside the playground.
    fn note_stamp(&mut self, name: &str, stamp: Option<SystemTime>) -> bool {
        self.stamps
            .insert(name.to_owned(), stamp)
            .is_some_and(|previous| previous != stamp)
    }
}

/// Whether the copy installed in `dir` was built from `package`'s entry and from
/// `sources` and `documentation`, as they are on disk now.
fn installed_copy_matches(
    dir: &Path,
    package: &PackageManifest,
    sources: &[ArtifactSource],
    documentation: &str,
) -> bool {
    let Ok(installed) = read_installed_sources(dir) else {
        return false;
    };
    let by_path = |sources: &mut Vec<ArtifactSource>| {
        sources.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    };
    let mut embedded = installed.sources;
    by_path(&mut embedded);
    let mut on_disk = sources.to_vec();
    by_path(&mut on_disk);
    let metadata = &installed.metadata;
    // An artifact stores its keywords sorted, without repeats.
    let mut keywords = package.keywords.clone();
    keywords.sort();
    keywords.dedup();
    // A dependency pinned by GitHub commit records the version it adopted, which the
    // entry does not declare.
    let dependencies_match = metadata.dependencies.len() == package.dependencies.len()
        && package.dependencies.iter().all(|declared| {
            metadata.dependencies.iter().any(|recorded| {
                recorded.name == declared.name.as_str()
                    && declared
                        .version
                        .as_ref()
                        .is_none_or(|version| recorded.version == version.as_str())
            })
        });
    embedded == on_disk
        && installed.documentation == documentation
        && metadata.package_version == package.version.as_str()
        && metadata.description == package.description
        && metadata.keywords == keywords
        && dependencies_match
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

/// A digest of what a build reads: the manifest text, each package's sources and
/// documentation, and when each package they depend on from the store was installed,
/// since installing a missing or changed dependency changes what the build reads.
fn fingerprint(manifest_text: &str, store: &PackageStore, inputs: &[PackageInputs<'_>]) -> u64 {
    use sha2::Digest;
    let mut digest = sha2::Sha256::default();
    let mut part = |part: &str| {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    };
    part(manifest_text);
    for input in inputs {
        part(input.package.name.as_str());
        part(&input.documentation);
        for source in &input.sources {
            part(source.path.as_str());
            part(&source.text);
        }
    }
    let dependencies: BTreeSet<&str> = inputs
        .iter()
        .flat_map(|input| &input.package.dependencies)
        .filter(|dependency| dependency.kind != DependencyKind::Sibling)
        .map(|dependency| dependency.name.as_str())
        .collect();
    for name in dependencies {
        part(name);
        let stamp = store
            .locate(name)
            .ok()
            .flatten()
            .and_then(|located| installed_at(&located.dir))
            .and_then(|stamp| stamp.duration_since(SystemTime::UNIX_EPOCH).ok());
        part(&format!("{stamp:?}"));
    }
    let digest = digest.finalize();
    let mut first = [0_u8; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(first)
}

/// The server's pre-execute hook: a run's project packages are brought up to date
/// before it compiles. The blueprint watcher brings the project packages an edit newly
/// names up to date through it too, before the edit is validated. Checks are
/// serialized, so two runs after one edit rebuild once.
pub(crate) struct Freshness {
    packages: Arc<ProjectPackages>,
    /// Held by the blocking task itself, so a check whose caller stops waiting still
    /// finishes before the next one starts.
    turn: Arc<tokio::sync::Mutex<()>>,
}

impl Freshness {
    pub(crate) fn new(packages: Arc<ProjectPackages>) -> Self {
        Self {
            packages,
            turn: Arc::default(),
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
    /// prepared packages when it replaced one. The eviction and the notes happen on
    /// that thread too, so a caller that stops waiting cannot skip them and leave a
    /// replaced package's cached copy to serve the next run.
    async fn check(
        &self,
        state: &AppState,
        check: impl FnOnce(&ProjectPackages) -> Result<Synced, ResolutionFailure> + Send + 'static,
    ) -> Result<(), ResolutionFailure> {
        let state = state.clone();
        self.run_check(move |packages| {
            let synced = check(packages)?;
            for name in &synced.reinstalled {
                note(&format!(
                    "reinstalled {name}: its source changed since it was installed"
                ));
            }
            if synced.evict {
                state.evict_all_prepared_packages();
            }
            Ok(synced)
        })
        .await
        .map(drop)
    }

    /// Run `check` on a blocking thread once every earlier check has finished. The
    /// turn moves into the blocking task, so dropping this future does not let the next
    /// check overlap a build that is still running.
    pub(super) async fn run_check(
        &self,
        check: impl FnOnce(&ProjectPackages) -> Result<Synced, ResolutionFailure> + Send + 'static,
    ) -> Result<Synced, ResolutionFailure> {
        let turn = Arc::clone(&self.turn).lock_owned().await;
        let packages = Arc::clone(&self.packages);
        tokio::task::spawn_blocking(move || {
            let _turn = turn;
            check(&packages)
        })
        .await
        .map_err(|error| ResolutionFailure::Stopped(error.to_string()))?
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
                message: failure.for_run(),
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

#[cfg(test)]
mod tests;
