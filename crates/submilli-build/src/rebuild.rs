//! Rebuild an installed package when the compiler that produced its wasm is
//! not the compiler now loading it.
//!
//! Codegen changes are assumed safe to replay. Unfinished durable runs therefore
//! load the rebuilt packages. Dropping the server version from
//! `runner::durable::COMPATIBILITY` belongs to SUB-1881, not this path.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use interpreter::{ModulePath, PackageSourceModule};
use serde::{Deserialize, Serialize};

use crate::artifact::{self, ArtifactError, ArtifactMetadata, ArtifactSource, PackageIdentity};
use crate::package_store::{PackageStoreError, package_dir_under};
use crate::{Artifact, CapabilitySchema};

const COMPLETE_MARKER: &str = "COMPLETE";
const CACHE_KEY_FILE: &str = "cache-key";

pub(crate) fn current_compiler() -> &'static str {
    interpreter::COMPILER_ID
}

pub(crate) fn needs_rebuild(metadata: &ArtifactMetadata) -> bool {
    metadata.schema_version != artifact::ARTIFACT_SCHEMA_VERSION
        || metadata.compiler.as_deref() != Some(current_compiler())
}

/// A package rebuilt from the sources retained in its artifact.
pub(crate) struct Recompiled {
    pub wasm: Vec<u8>,
    pub type_info: interpreter::TypeInfoTable,
    pub capabilities: CapabilitySchema,
    pub declaration: interpreter::PackageDeclaration,
    pub metadata: ArtifactMetadata,
    pub documentation: String,
    pub sources: Vec<ArtifactSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RecordedFailure {
    compiler: String,
    /// Hash of the sources the failure was compiled from. A later install with
    /// different sources retries instead of keeping this error.
    sources: String,
    /// Direct dependencies as they were when the compile failed. A rebuilt
    /// dependency must retry this package even if its own sources are unchanged.
    dependencies: String,
    detail: String,
}

pub(crate) fn recorded_failure(
    store_root: &Path,
    package_name: &str,
    sources: &[ArtifactSource],
    dependencies: &str,
) -> Result<Option<String>, PackageStoreError> {
    let path = failure_path(store_root, package_name)?;
    let Some(bytes) = read_optional(&path)? else {
        return Ok(None);
    };
    let Ok(recorded) = serde_json::from_slice::<RecordedFailure>(&bytes) else {
        return Ok(None);
    };
    if recorded.compiler == current_compiler()
        && recorded.sources == source_hash(sources)
        && recorded.dependencies == dependencies
    {
        Ok(Some(recorded.detail))
    } else {
        Ok(None)
    }
}

pub(crate) fn record_failure(
    store_root: &Path,
    package_name: &str,
    sources: &[ArtifactSource],
    dependencies: &str,
    detail: &str,
) -> Result<(), PackageStoreError> {
    let path = failure_path(store_root, package_name)?;
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    let bytes = serde_json::to_vec(&RecordedFailure {
        compiler: current_compiler().to_string(),
        sources: source_hash(sources),
        dependencies: dependencies.to_string(),
        detail: detail.to_string(),
    })
    .map_err(|source| io_error(&path, io::Error::new(io::ErrorKind::InvalidData, source)))?;
    write_atomic(&path, &bytes)
}

/// Identity of the direct dependencies a failed compile saw. A later rebuild of
/// one of them changes this key, so the dependent is compiled again.
pub(crate) fn cache_key(
    sources: &[ArtifactSource],
    dependency_key: &str,
    overrides: &std::collections::BTreeMap<String, String>,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source_hash(sources).as_bytes());
    hasher.update([0]);
    hasher.update(dependency_key.as_bytes());
    hasher.update([0]);
    for (name, value) in overrides {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn dependency_key(dependencies: &[Artifact]) -> String {
    use sha2::{Digest, Sha256};
    let mut ordered: Vec<&Artifact> = dependencies.iter().collect();
    ordered.sort_by(|left, right| left.metadata.package_name.cmp(&right.metadata.package_name));
    let mut hasher = Sha256::new();
    for artifact in ordered {
        hasher.update(artifact.metadata.package_name.as_bytes());
        hasher.update([0]);
        hasher.update(artifact.metadata.package_version.as_bytes());
        hasher.update([0]);
        hasher.update(
            artifact
                .metadata
                .compiler
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(source_hash(&artifact.sources).as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn source_hash(sources: &[ArtifactSource]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut ordered: Vec<&ArtifactSource> = sources.iter().collect();
    ordered.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
    for source in ordered {
        hasher.update(source.path.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(source.text.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn clear_failure(
    store_root: &Path,
    package_name: &str,
) -> Result<(), PackageStoreError> {
    let path = failure_path(store_root, package_name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(&path, source)),
    }
}

/// Finish or roll back a publish that crashed between the two renames.
///
/// A reader then sees the previous complete artifact or the staged one, never
/// a directory that mixes the two. Called under [`PackageLock`].
/// Run `body` while this process holds the package's publish lock. Install and
/// removal use it so they cannot create the live directory in the gap between
/// the two publish renames.
/// Drop crash-recovery copies so a delete cannot be undone by the next load.
pub fn discard_rebuild_state(
    store_root: &Path,
    package_name: &str,
) -> Result<(), PackageStoreError> {
    for slot in [OWNED_SLOT, CACHE_SLOT] {
        remove_dir(&previous_dir(store_root, package_name, slot)?)?;
        remove_dir(&staging_parent(store_root, package_name, slot)?)?;
    }
    remove_dir(&cache_dir(store_root, package_name)?)?;
    clear_failure(store_root, package_name)
}

pub fn with_package_lock<T>(
    store_root: &Path,
    package_name: &str,
    body: impl FnOnce() -> Result<T, PackageStoreError>,
) -> Result<T, PackageStoreError> {
    let _lock = PackageLock::exclusive(&lock_path(store_root, package_name)?)?;
    body()
}

/// Recover while holding the package lock. Publishers call [`recover`] only
/// after they already hold that lock; a second lock in this process would
/// unlock the first when dropped.
pub(crate) fn recover_exclusive(
    store_root: &Path,
    package_name: &str,
) -> Result<(), PackageStoreError> {
    if !interrupted_publish(store_root, package_name)? {
        return Ok(());
    }
    let _lock = PackageLock::exclusive(&lock_path(store_root, package_name)?)?;
    recover(store_root, package_name)
}

fn interrupted_publish(store_root: &Path, package_name: &str) -> Result<bool, PackageStoreError> {
    for slot in [OWNED_SLOT, CACHE_SLOT] {
        if previous_dir(store_root, package_name, slot)?.exists()
            || staging_parent(store_root, package_name, slot)?.exists()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn recover(store_root: &Path, package_name: &str) -> Result<(), PackageStoreError> {
    let live = package_dir_under(store_root, package_name)?;
    recover_at(store_root, package_name, &live, OWNED_SLOT)?;
    recover_at(
        store_root,
        package_name,
        &cache_dir(store_root, package_name)?,
        CACHE_SLOT,
    )
}

const OWNED_SLOT: &str = "owned";
const CACHE_SLOT: &str = "cache";

/// Rebuilt copy of a fallback package. Not a package directory, so it does not
/// shadow the fallback or undo a delete of the owned copy.
pub(crate) fn cache_dir(
    store_root: &Path,
    package_name: &str,
) -> Result<PathBuf, PackageStoreError> {
    let (scope, leaf) = split(package_name)?;
    Ok(store_root
        .join(".rebuild")
        .join("cache")
        .join(scope)
        .join(leaf))
}

fn recover_at(
    store_root: &Path,
    package_name: &str,
    live: &Path,
    slot: &str,
) -> Result<(), PackageStoreError> {
    let previous = previous_dir(store_root, package_name, slot)?;
    let stagings = staging_dirs(store_root, package_name, slot)?;
    if artifact::artifact_dir_is_complete(live) {
        remove_dir(&previous)?;
        for staging in stagings {
            remove_dir(&staging)?;
        }
        return Ok(());
    }
    let mut complete = stagings
        .iter()
        .filter(|staging| {
            staging.join(COMPLETE_MARKER).is_file() && artifact::artifact_dir_is_complete(staging)
        })
        .cloned()
        .collect::<Vec<_>>();
    complete.sort();
    if let Some(staging) = complete.first() {
        replace_dir(staging, live)?;
        remove_dir(&previous)?;
        for extra in &stagings {
            if extra != staging {
                remove_dir(extra)?;
            }
        }
        let _ = fs::remove_file(live.join(COMPLETE_MARKER));
        return Ok(());
    }
    if artifact::artifact_dir_is_complete(&previous) {
        replace_dir(&previous, live)?;
    }
    Ok(())
}

pub(crate) enum PublishOutcome {
    Published,
    AlreadyCurrent,
    /// The owned directory was removed while this package was compiling.
    /// Do not recreate it.
    Removed,
}

/// Write `compiled` into `live` so a crash leaves the old directory or the new
/// one. Another publisher that already installed the current stamp wins; this
/// call then discards its staging directory.
pub(crate) fn publish(
    store_root: &Path,
    package_name: &str,
    live: &Path,
    compiled: &Recompiled,
    slot: &str,
    cache_key: Option<&str>,
) -> Result<PublishOutcome, PackageStoreError> {
    let _lock = PackageLock::exclusive(&lock_path(store_root, package_name)?)?;
    publish_locked(
        store_root,
        package_name,
        live,
        compiled,
        slot,
        false,
        cache_key,
    )
}

/// Replace `live` even when its stamp already matches. Install uses this so a
/// same-compiler reinstall still writes the new sources, and so the write is
/// the same atomic swap as a rebuild.
pub(crate) fn replace_installed(
    store_root: &Path,
    package_name: &str,
    live: &Path,
    compiled: &Recompiled,
) -> Result<(), PackageStoreError> {
    let _lock = PackageLock::exclusive(&lock_path(store_root, package_name)?)?;
    match publish_locked(
        store_root,
        package_name,
        live,
        compiled,
        OWNED_SLOT,
        true,
        None,
    )? {
        PublishOutcome::Removed => Err(PackageStoreError::MissingPackage {
            name: package_name.to_string(),
            searched_roots: vec![store_root.to_path_buf()],
            available: Vec::new(),
        }),
        PublishOutcome::Published | PublishOutcome::AlreadyCurrent => Ok(()),
    }
}

fn publish_locked(
    store_root: &Path,
    package_name: &str,
    live: &Path,
    compiled: &Recompiled,
    slot: &str,
    force: bool,
    cache_key: Option<&str>,
) -> Result<PublishOutcome, PackageStoreError> {
    recover_at(store_root, package_name, live, slot)?;
    // A rebuild must not recreate an owned package removed while it compiled.
    // Install passes `force` and is allowed to create the directory.
    if !force
        && slot == OWNED_SLOT
        && !live.exists()
        && !previous_dir(store_root, package_name, slot)?.exists()
    {
        return Ok(PublishOutcome::Removed);
    }
    if !force && live_is_current(live)? && cache_key_matches(live, cache_key) {
        return Ok(PublishOutcome::AlreadyCurrent);
    }
    let staging = staging_dir(store_root, package_name, slot)?;
    if staging.exists() {
        remove_dir(&staging)?;
    }
    if let Some(parent) = staging.parent() {
        create_dir(parent)?;
    }
    artifact::write_package_artifact_with_docs_and_sources(
        &staging,
        &compiled.wasm,
        &compiled.type_info,
        &compiled.capabilities,
        &compiled.declaration,
        &compiled.metadata,
        &compiled.documentation,
        &compiled.sources,
    )
    .map_err(|source| PackageStoreError::Artifact {
        name: package_name.to_string(),
        package_dir: staging.clone(),
        source,
    })?;
    if let Some(key) = cache_key {
        write_atomic(&staging.join(CACHE_KEY_FILE), key.as_bytes())?;
    }
    sync_tree(&staging)?;
    write_atomic(&staging.join(COMPLETE_MARKER), b"ok")?;
    let previous = previous_dir(store_root, package_name, slot)?;
    if live.exists() {
        if previous.exists() {
            remove_dir(&previous)?;
        }
        if let Some(parent) = previous.parent() {
            create_dir(parent)?;
        }
        rename(live, &previous)?;
    }
    // A fallback-only package has no owned `@scope` directory yet. `rename`
    // does not create the destination's parent.
    if let Some(parent) = live.parent() {
        create_dir(parent)?;
    }
    if let Err(error) = rename(&staging, live) {
        if previous.exists() && !live.exists() {
            let _ = fs::rename(&previous, live);
        }
        return Err(error);
    }
    let _ = fs::remove_file(live.join(COMPLETE_MARKER));
    remove_dir(&previous)?;
    // An empty staging parent would look like an interrupted publish and make
    // the next read take the lock.
    if let Some(parent) = staging.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(PublishOutcome::Published)
}

pub(crate) fn recompile(
    identity: &PackageIdentity,
    dependencies: &[Artifact],
) -> Result<Recompiled, String> {
    if identity.sources.is_empty() {
        return Err("sources are missing; refusing to run the installed wasm".to_string());
    }
    let direct_names: Vec<&str> = identity
        .metadata
        .dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect();
    let direct: Vec<&interpreter::PackageDeclaration> = dependencies
        .iter()
        .filter(|artifact| direct_names.contains(&artifact.metadata.package_name.as_str()))
        .map(|artifact| &artifact.package_declaration)
        .collect();
    let transitive: Vec<&interpreter::PackageDeclaration> = dependencies
        .iter()
        .filter(|artifact| !direct_names.contains(&artifact.metadata.package_name.as_str()))
        .map(|artifact| &artifact.package_declaration)
        .collect();
    let modules: Vec<PackageSourceModule<'_>> = identity
        .sources
        .iter()
        .map(|source| PackageSourceModule {
            path: source.path.clone(),
            source: &source.text,
        })
        .collect();
    let overrides = &identity.metadata.config.overrides;
    let config = resolved_config(&identity.sources, overrides)?;
    let compiled = interpreter::compile::compile_package_configured(
        interpreter::Stdlib::core(),
        &identity.metadata.package_name,
        ModulePath::from("lib"),
        &modules,
        &direct,
        &transitive,
        overrides,
        interpreter::PackageSurface::Public,
    )
    .map_err(|error| render_compile_error(&identity.sources, error))?;
    let stdlib_declarations = interpreter::runtime::stdlib_package_declarations();
    let (prelude_declarations, host_declarations, _) =
        interpreter::runtime::prelude::cached_runtime_package_declarations();
    let schema_dependencies: Vec<&interpreter::PackageDeclaration> = direct
        .iter()
        .chain(&transitive)
        .copied()
        .chain(&stdlib_declarations)
        .chain(prelude_declarations)
        .chain(host_declarations)
        .collect();
    let capabilities = crate::derive_capability_schema(
        &compiled.declaration,
        &schema_dependencies,
        &compiled.required_capabilities,
    );
    let mut metadata = identity.metadata.clone();
    metadata.schema_version = artifact::ARTIFACT_SCHEMA_VERSION;
    metadata.compiler = Some(current_compiler().to_string());
    metadata.config = config;
    Ok(Recompiled {
        wasm: compiled.wasm,
        type_info: compiled.type_info,
        capabilities,
        declaration: compiled.declaration,
        metadata,
        documentation: identity.documentation.clone(),
        sources: identity.sources.clone(),
    })
}

fn resolved_config(
    sources: &[ArtifactSource],
    overrides: &std::collections::BTreeMap<String, String>,
) -> Result<crate::PackageConfig, String> {
    let text = sources
        .iter()
        .find(|source| source.path.as_str() == "config")
        .map_or("", |source| source.text.as_str());
    let file = interpreter::FileId(0);
    let parameters = interpreter::package_config::parameters(text, file)
        .map_err(|error| render_compile_error(sources, error))?;
    let values = interpreter::package_config::resolve(&parameters, overrides, file)
        .map_err(|error| render_compile_error(sources, error))?;
    Ok(crate::PackageConfig {
        overrides: overrides.clone(),
        values,
    })
}

fn render_compile_error(
    sources: &[ArtifactSource],
    error: interpreter::compiler_error::CompileError,
) -> String {
    let diagnostics = error.into_diagnostics(interpreter::FileId(0));
    let mut registry = interpreter::Sources::new();
    for source in sources {
        if registry
            .add(source.path.clone(), source.text.clone())
            .is_err()
        {
            return diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    interpreter::diagnostics::render_collection(&diagnostics, &registry).map_or_else(
        |_| {
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>()
                .join("\n")
        },
        |rendered| rendered.text,
    )
}

fn live_is_current(live: &Path) -> Result<bool, PackageStoreError> {
    if !artifact::artifact_dir_is_complete(live) {
        return Ok(false);
    }
    // Unreadable metadata is not current. Treating it as an error would leave a
    // corrupt cache in place instead of replacing it.
    match artifact::read_package_identity(live) {
        Ok(identity) => Ok(!needs_rebuild(&identity.metadata)),
        Err(_) => Ok(false),
    }
}

fn cache_key_matches(live: &Path, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    std::fs::read_to_string(live.join(CACHE_KEY_FILE)).is_ok_and(|found| found == expected)
}

fn staging_dirs(
    store_root: &Path,
    package_name: &str,
    slot: &str,
) -> Result<Vec<PathBuf>, PackageStoreError> {
    let parent = staging_parent(store_root, package_name, slot)?;
    let Ok(entries) = fs::read_dir(&parent) else {
        return Ok(Vec::new());
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| io_error(&parent, source))?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn replace_dir(from: &Path, to: &Path) -> Result<(), PackageStoreError> {
    if to.exists() {
        remove_dir(to)?;
    }
    if let Some(parent) = to.parent() {
        create_dir(parent)?;
    }
    rename(from, to)
}

fn staging_dir(
    store_root: &Path,
    package_name: &str,
    slot: &str,
) -> Result<PathBuf, PackageStoreError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    Ok(staging_parent(store_root, package_name, slot)?
        .join(format!("{nanos}-{}", std::process::id())))
}

/// One directory per package and publish slot, so `@scope/http` cannot see
/// `@scope/http-client`'s staging and a cache publish cannot restore into the
/// owned package directory.
fn staging_parent(
    store_root: &Path,
    package_name: &str,
    slot: &str,
) -> Result<PathBuf, PackageStoreError> {
    let (scope, leaf) = split(package_name)?;
    Ok(store_root
        .join(".rebuild")
        .join("staging")
        .join(slot)
        .join(scope)
        .join(leaf))
}

fn previous_dir(
    store_root: &Path,
    package_name: &str,
    slot: &str,
) -> Result<PathBuf, PackageStoreError> {
    let (scope, leaf) = split(package_name)?;
    Ok(store_root
        .join(".rebuild")
        .join("previous")
        .join(slot)
        .join(scope)
        .join(leaf))
}

fn failure_path(store_root: &Path, package_name: &str) -> Result<PathBuf, PackageStoreError> {
    let (scope, leaf) = split(package_name)?;
    Ok(store_root
        .join(".rebuild")
        .join("failures")
        .join(scope)
        .join(format!("{leaf}.json")))
}

fn lock_path(store_root: &Path, package_name: &str) -> Result<PathBuf, PackageStoreError> {
    let (scope, leaf) = split(package_name)?;
    Ok(store_root
        .join(".rebuild")
        .join("locks")
        .join(scope)
        .join(format!("{leaf}.lock")))
}

fn split(package_name: &str) -> Result<(&str, &str), PackageStoreError> {
    crate::package_store::split_scoped_name(package_name).ok_or_else(|| {
        PackageStoreError::InvalidPackageName {
            name: package_name.to_string(),
        }
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PackageStoreError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    let temporary = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|source| io_error(&temporary, source))?;
        file.write_all(bytes)
            .map_err(|source| io_error(&temporary, source))?;
        file.sync_all()
            .map_err(|source| io_error(&temporary, source))?;
    }
    // Windows `rename` does not replace an existing file.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(io_error(path, source)),
    }
    rename(&temporary, path)
}

/// File contents are synced before the completeness marker. Directory sync is
/// not portable (Windows refuses to open a directory this way) and is only a
/// durability hint for power loss, so a failure there does not fail the publish.
fn sync_tree(path: &Path) -> Result<(), PackageStoreError> {
    let entries = fs::read_dir(path).map_err(|source| io_error(path, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_error(path, source))?;
        let child = entry.path();
        if child.is_dir() {
            sync_tree(&child)?;
        } else {
            let file = File::open(&child).map_err(|source| io_error(&child, source))?;
            file.sync_all().map_err(|source| io_error(&child, source))?;
        }
    }
    if let Ok(file) = File::open(path) {
        let _ = file.sync_all();
    }
    Ok(())
}

fn rename(from: &Path, to: &Path) -> Result<(), PackageStoreError> {
    fs::rename(from, to).map_err(|source| io_error(from, source))
}

fn create_dir(path: &Path) -> Result<(), PackageStoreError> {
    fs::create_dir_all(path).map_err(|source| io_error(path, source))
}

fn remove_dir(path: &Path) -> Result<(), PackageStoreError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PackageStoreError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(path, source)),
    }
}

fn io_error(path: &Path, source: impl Into<io::Error>) -> PackageStoreError {
    PackageStoreError::Artifact {
        name: path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        package_dir: path.to_path_buf(),
        source: ArtifactError::Io {
            path: path.to_path_buf(),
            source: source.into(),
        },
    }
}

/// Exclusive lock released on drop, and by the kernel if the process dies.
/// Crash recovery depends on that: a dead publisher must not keep the lock.
struct PackageLock {
    file: File,
}

impl PackageLock {
    fn exclusive(path: &Path) -> Result<Self, PackageStoreError> {
        if let Some(parent) = path.parent() {
            create_dir(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|source| io_error(path, source))?;
        lock_exclusive(&file, path)?;
        Ok(Self { file })
    }
}

impl Drop for PackageLock {
    fn drop(&mut self) {
        unlock_exclusive(&self.file);
    }
}

#[cfg(unix)]
fn lock_exclusive(file: &File, path: &Path) -> Result<(), PackageStoreError> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: `file` is a live descriptor. `LOCK_EX` blocks until this process
    // holds the lock. The kernel drops it if the process exits, including a
    // crash between the publish renames.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io_error(path, io::Error::last_os_error()))
    }
}

#[cfg(unix)]
fn unlock_exclusive(file: &File) {
    use std::os::unix::io::AsRawFd;
    // SAFETY: the same live descriptor `lock_exclusive` locked. Unlocking a
    // lock this guard holds has no recovery if it fails; process exit releases it.
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

#[cfg(windows)]
#[repr(C)]
struct WindowsOverlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut core::ffi::c_void,
}

#[cfg(windows)]
impl Default for WindowsOverlapped {
    fn default() -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
fn lock_exclusive(file: &File, path: &Path) -> Result<(), PackageStoreError> {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = WindowsOverlapped::default();
    // SAFETY: `file` is a live handle and `overlapped` is a live zeroed
    // `OVERLAPPED`. Exclusive, blocking, whole-file. The kernel releases it if
    // the process exits.
    let ok = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            0x0000_0002,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if ok == 0 {
        Err(io_error(path, io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn unlock_exclusive(file: &File) {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = WindowsOverlapped::default();
    unsafe {
        UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LockFileEx(
        file: *mut core::ffi::c_void,
        flags: u32,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut WindowsOverlapped,
    ) -> i32;
    fn UnlockFileEx(
        file: *mut core::ffi::c_void,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut WindowsOverlapped,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::thread;
    use std::time::SystemTime;

    use interpreter::{ModulePath, PackageDeclaration};

    use super::*;
    use crate::artifact::{
        ArtifactSource, artifact_dir_is_complete, write_package_artifact_with_docs_and_sources,
    };
    use crate::package_store::{PackageStore, PackageStoreError, package_dir_under};
    use crate::{ArtifactDependency, ArtifactMetadata, derive_capability_schema};

    const LIB: &str = "export function hello(): string {\n    return \"hello\";\n}\n";

    fn write_stale(
        root: &Path,
        name: &str,
        version: &str,
        sources: &[(&str, &str)],
        dependencies: &[(&str, &str)],
        compiler: Option<&str>,
    ) {
        let dir = package_dir_under(root, name).expect("package dir");
        let mut declaration = PackageDeclaration::with_package(name);
        declaration.refresh_shapes();
        let mut metadata = ArtifactMetadata::new(
            name,
            version,
            dependencies
                .iter()
                .map(|(dependency, version)| ArtifactDependency::new(*dependency, *version))
                .collect(),
        );
        metadata.compiler = compiler.map(str::to_string);
        let sources = sources
            .iter()
            .map(|(path, text)| ArtifactSource {
                path: ModulePath::from(*path),
                text: (*text).to_string(),
            })
            .collect::<Vec<_>>();
        let type_info = interpreter::TypeInfoTable {
            package_name: name.to_string(),
            types: Vec::new(),
        };
        write_package_artifact_with_docs_and_sources(
            dir,
            b"STALE-WASM",
            &type_info,
            &derive_capability_schema(&declaration, &[], &[]),
            &declaration,
            &metadata,
            "readme",
            &sources,
        )
        .expect("write stale artifact");
    }

    fn wasm_mtime(root: &Path, name: &str) -> SystemTime {
        fs::metadata(package_dir_under(root, name).unwrap().join("pkg.wasm"))
            .unwrap()
            .modified()
            .unwrap()
    }

    #[test]
    fn a_fallback_only_package_is_published_into_the_owned_store() {
        let owned = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        write_stale(
            fallback.path(),
            "@t/lib",
            "1.0.0",
            &[("lib", LIB)],
            &[],
            None,
        );
        let store = PackageStore::new(owned.path()).with_fallback(fallback.path());

        let loaded = store
            .load("@t/lib")
            .expect("first load publishes into the owned store");

        assert_eq!(
            loaded.metadata.compiler.as_deref(),
            Some(current_compiler())
        );
        assert!(
            !owned.path().join("@t").join("lib").exists(),
            "a fallback rebuild must not shadow the fallback"
        );
        assert!(
            cache_dir(owned.path(), "@t/lib")
                .unwrap()
                .join("pkg.wasm")
                .is_file()
        );
        assert_eq!(
            fs::read(fallback.path().join("@t").join("lib").join("pkg.wasm")).unwrap(),
            b"STALE-WASM"
        );
        let again = store.load("@t/lib").expect("cached rebuild");
        assert_eq!(again.wasm, loaded.wasm);
    }

    #[test]
    fn an_older_stamp_is_recompiled_once() {
        let root = tempfile::tempdir().unwrap();
        write_stale(
            root.path(),
            "@t/lib",
            "1.0.0",
            &[("lib", LIB)],
            &[],
            Some("0.0.0+old"),
        );
        let store = PackageStore::new(root.path());

        let first = store.load("@t/lib").expect("rebuild on load");
        assert_eq!(first.metadata.compiler.as_deref(), Some(current_compiler()));
        assert_ne!(first.wasm, b"STALE-WASM");
        let mtime = wasm_mtime(root.path(), "@t/lib");

        let second = store.load("@t/lib").expect("second load");
        assert_eq!(second.wasm, first.wasm);
        assert_eq!(wasm_mtime(root.path(), "@t/lib"), mtime);
        assert!(store.reconcile_installed().rebuilt.is_empty());
    }

    #[test]
    fn a_missing_stamp_is_recompiled_at_startup() {
        let root = tempfile::tempdir().unwrap();
        write_stale(root.path(), "@t/lib", "1.0.0", &[("lib", LIB)], &[], None);
        let store = PackageStore::new(root.path());

        let report = store.reconcile_installed();
        assert_eq!(report.rebuilt, vec!["@t/lib".to_string()]);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let loaded = store.load("@t/lib").expect("load after startup");
        assert_eq!(
            loaded.metadata.compiler.as_deref(),
            Some(current_compiler())
        );
        assert_ne!(loaded.wasm, b"STALE-WASM");
    }

    #[test]
    fn a_cycle_through_a_current_package_is_an_error_not_a_stack_overflow() {
        let root = tempfile::tempdir().unwrap();
        write_stale(
            root.path(),
            "@t/new",
            "1.0.0",
            &[("lib", LIB)],
            &[("@t/old", "1.0.0")],
            Some(current_compiler()),
        );
        write_stale(
            root.path(),
            "@t/old",
            "1.0.0",
            &[("lib", LIB)],
            &[("@t/new", "1.0.0")],
            None,
        );
        let store = PackageStore::new(root.path());

        let error = store.load("@t/old").expect_err("cycle");
        assert!(
            matches!(error, PackageStoreError::DependencyCycle { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_current_middle_package_still_supplies_its_dependency() {
        let root = tempfile::tempdir().unwrap();
        write_stale(root.path(), "@t/leaf", "1.0.0", &[("lib", LIB)], &[], None);
        write_stale(
            root.path(),
            "@t/mid",
            "1.0.0",
            &[(
                "lib",
                "import { hello } from \"@t/leaf\";\nexport function mid(): string {\n    return hello();\n}\n",
            )],
            &[("@t/leaf", "1.0.0")],
            None,
        );
        let store = PackageStore::new(root.path());
        store.reconcile_installed();
        write_stale(
            root.path(),
            "@t/top",
            "1.0.0",
            &[(
                "lib",
                "import { mid } from \"@t/mid\";\nexport function top(): string {\n    return mid();\n}\n",
            )],
            &[("@t/mid", "1.0.0")],
            Some("0.0.0+old"),
        );

        let loaded = store
            .load("@t/top")
            .expect("top rebuilds against the current middle package");
        assert_eq!(
            loaded.metadata.compiler.as_deref(),
            Some(current_compiler())
        );
        assert_ne!(loaded.wasm, b"STALE-WASM");
    }

    #[test]
    fn a_dependency_chain_rebuilds_in_order_and_a_broken_package_does_not_hide_the_others() {
        let root = tempfile::tempdir().unwrap();
        write_stale(root.path(), "@t/dep", "1.0.0", &[("lib", LIB)], &[], None);
        write_stale(
            root.path(),
            "@t/app",
            "1.0.0",
            &[(
                "lib",
                "import { hello } from \"@t/dep\";\nexport function greet(): string {\n    return hello();\n}\n",
            )],
            &[("@t/dep", "1.0.0")],
            None,
        );
        write_stale(
            root.path(),
            "@t/bad",
            "1.0.0",
            &[("lib", "export function broken( {\n")],
            &[],
            None,
        );
        write_stale(
            root.path(),
            "@t/uses-bad",
            "1.0.0",
            &[(
                "lib",
                "import { broken } from \"@t/bad\";\nexport function go(): string {\n    return \"no\";\n}\n",
            )],
            &[("@t/bad", "1.0.0")],
            None,
        );
        write_stale(root.path(), "@t/ok", "1.0.0", &[("lib", LIB)], &[], None);
        let store = PackageStore::new(root.path());

        let report = store.reconcile_installed();
        let dep = report
            .rebuilt
            .iter()
            .position(|name| name == "@t/dep")
            .unwrap();
        let app = report
            .rebuilt
            .iter()
            .position(|name| name == "@t/app")
            .unwrap();
        assert!(dep < app, "rebuilt {:?}", report.rebuilt);
        assert!(report.rebuilt.iter().any(|name| name == "@t/ok"));
        assert!(!report.rebuilt.iter().any(|name| name == "@t/bad"));
        assert!(!report.rebuilt.iter().any(|name| name == "@t/uses-bad"));
        let bad = report
            .failures
            .iter()
            .find(|failure| failure.name == "@t/bad")
            .expect("bad package reported");
        assert!(
            bad.compiler.contains(env!("CARGO_PKG_VERSION")),
            "{}",
            bad.compiler
        );
        assert!(
            bad.detail.contains("broken") || !bad.detail.is_empty(),
            "{}",
            bad.detail
        );
        let dependent = report
            .failures
            .iter()
            .find(|failure| failure.name == "@t/uses-bad")
            .expect("dependent reported");
        assert!(dependent.detail.contains("@t/bad"), "{}", dependent.detail);

        assert!(store.load("@t/ok").is_ok());
        let error = store
            .load("@t/bad")
            .expect_err("broken package stays unavailable");
        let text = error.to_string();
        assert!(text.contains("@t/bad"), "{text}");
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
        assert!(matches!(error, PackageStoreError::Unavailable { .. }));
        assert_eq!(
            fs::read(
                package_dir_under(root.path(), "@t/bad")
                    .unwrap()
                    .join("pkg.wasm")
            )
            .unwrap(),
            b"STALE-WASM"
        );
    }

    #[test]
    fn missing_sources_refuse_the_stale_wasm() {
        let root = tempfile::tempdir().unwrap();
        write_stale(root.path(), "@t/bare", "1.0.0", &[], &[], None);
        let store = PackageStore::new(root.path());

        let error = store.load("@t/bare").expect_err("missing sources");
        let text = error.to_string();
        assert!(text.contains("@t/bare"), "{text}");
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
        assert!(text.contains("sources are missing"), "{text}");
        assert_eq!(
            fs::read(
                package_dir_under(root.path(), "@t/bare")
                    .unwrap()
                    .join("pkg.wasm")
            )
            .unwrap(),
            b"STALE-WASM"
        );
    }

    #[test]
    fn a_crash_between_renames_leaves_a_complete_artifact() {
        let root = tempfile::tempdir().unwrap();
        write_stale(
            root.path(),
            "@t/lib",
            "1.0.0",
            &[("lib", LIB)],
            &[],
            Some(current_compiler()),
        );
        let live = package_dir_under(root.path(), "@t/lib").unwrap();
        let previous = previous_dir(root.path(), "@t/lib", OWNED_SLOT).unwrap();
        fs::create_dir_all(previous.parent().unwrap()).unwrap();
        fs::rename(&live, &previous).unwrap();
        let staging = staging_parent(root.path(), "@t/lib", OWNED_SLOT)
            .unwrap()
            .join("crash");
        fs::create_dir_all(&staging).unwrap();
        fs::copy(
            previous.join("metadata.json"),
            staging.join("metadata.json"),
        )
        .unwrap();
        fs::copy(previous.join("pkg.wasm"), staging.join("pkg.wasm")).unwrap();
        fs::write(staging.join("pkg.wasm"), b"NEW-WASM").unwrap();
        for name in [
            "type-info.json",
            "capabilities.yaml",
            "package-declaration.json",
        ] {
            fs::copy(previous.join(name), staging.join(name)).unwrap();
        }
        fs::write(staging.join(COMPLETE_MARKER), b"ok").unwrap();

        recover(root.path(), "@t/lib").unwrap();

        assert!(artifact_dir_is_complete(&live));
        assert_eq!(fs::read(live.join("pkg.wasm")).unwrap(), b"NEW-WASM");
        assert!(!previous.exists());
        assert!(!staging.exists());
    }

    #[test]
    fn an_incomplete_staging_directory_keeps_the_old_artifact() {
        let root = tempfile::tempdir().unwrap();
        write_stale(
            root.path(),
            "@t/lib",
            "1.0.0",
            &[("lib", LIB)],
            &[],
            Some(current_compiler()),
        );
        let live = package_dir_under(root.path(), "@t/lib").unwrap();
        let staging = staging_parent(root.path(), "@t/lib", OWNED_SLOT)
            .unwrap()
            .join("partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("pkg.wasm"), b"PARTIAL").unwrap();

        recover(root.path(), "@t/lib").unwrap();

        assert_eq!(fs::read(live.join("pkg.wasm")).unwrap(), b"STALE-WASM");
        assert!(artifact_dir_is_complete(&live));
        assert!(!staging.exists());
    }

    #[test]
    fn concurrent_publish_leaves_one_complete_artifact() {
        let root = tempfile::tempdir().unwrap();
        write_stale(
            root.path(),
            "@t/lib",
            "1.0.0",
            &[("lib", LIB)],
            &[],
            Some("0.0.0+old"),
        );
        let store = PackageStore::new(root.path());
        let identity =
            artifact::read_package_identity(&package_dir_under(root.path(), "@t/lib").unwrap())
                .unwrap();
        let compiled = recompile(&identity, &[]).unwrap();
        let first = Arc::new(compiled);
        let mut second = first.wasm.clone();
        second.extend_from_slice(b"-other");
        let mut other = Recompiled {
            wasm: second,
            type_info: first.type_info.clone(),
            capabilities: first.capabilities.clone(),
            declaration: first.declaration.clone(),
            metadata: first.metadata.clone(),
            documentation: first.documentation.clone(),
            sources: first.sources.clone(),
        };
        other.metadata.compiler = Some(current_compiler().to_string());
        let live = package_dir_under(root.path(), "@t/lib").unwrap();
        let root_path = root.path().to_path_buf();
        let left = {
            let compiled = Recompiled {
                wasm: first.wasm.clone(),
                type_info: first.type_info.clone(),
                capabilities: first.capabilities.clone(),
                declaration: first.declaration.clone(),
                metadata: first.metadata.clone(),
                documentation: first.documentation.clone(),
                sources: first.sources.clone(),
            };
            let root_path = root_path.clone();
            let live = live.clone();
            thread::spawn(move || {
                publish(&root_path, "@t/lib", &live, &compiled, OWNED_SLOT, None).unwrap()
            })
        };
        let right = thread::spawn(move || {
            publish(&root_path, "@t/lib", &live, &other, OWNED_SLOT, None).unwrap()
        });
        left.join().unwrap();
        right.join().unwrap();

        let artifact = store.load("@t/lib").unwrap();
        assert!(artifact.wasm == first.wasm || artifact.wasm.ends_with(b"-other"));
        assert!(artifact_dir_is_complete(
            &package_dir_under(root.path(), "@t/lib").unwrap()
        ));
        assert_eq!(
            artifact.metadata.compiler.as_deref(),
            Some(current_compiler())
        );
    }
}
