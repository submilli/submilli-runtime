//! Throwaway local state for a test run: copies of the files and session data the run
//! would touch, so it runs live against them and leaves the originals alone.
//!
//! The source session's `per_session` directory and each writable named volume the
//! blueprint mounts are copied into a temporary directory (a clone where the filesystem
//! has one, else a plain copy), up to [`LOCAL_STATE_CAP_BYTES`]. The session's `submilli:session`
//! data is read through [`ForkedSessionKv`]. Everything goes when the [`Throwaway`] drops.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(target_os = "macos")]
use interpreter::runtime::MAX_MEASURED_DEPTH;
use interpreter::runtime::session_kv::{
    InMemorySessionKv, SessionKvEntry, SessionKvError, SessionKvLimits, SessionKvPage,
    SessionKvStore,
};
use interpreter::runtime::{CopyDirError, copy_host_dir, measure_host_dir_skipping_vanished};
use serde::Serialize;
use submilli_blueprint::{Blueprint, VarBindings, VfsConfig};

use crate::config::{Access, VolumeKind, VolumeSpec, VolumeTable};
use crate::session_manager::SessionManager;
use crate::volumes::VolumeRegistry;

/// The most local state, in bytes, a test run copies: the source session's files and the
/// writable volumes the blueprint mounts, together. A run over more is refused.
pub const LOCAL_STATE_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// What a test run's local state was made from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LocalState {
    /// The source session's files and data are read as they are now, not as the recorded
    /// run saw them.
    pub as_of_now: bool,
    /// The source session was still there to copy from.
    pub session_found: bool,
    /// Volumes copied, because the run may write to them. A read-only volume is shared.
    pub volumes_copied: Vec<String>,
    pub bytes_copied: u64,
}

/// Why local state could not be copied.
#[derive(Debug)]
pub enum ThrowawayError {
    /// More than the cap; the message names it. `measured` is the whole size, when the
    /// sources were measured before any was copied.
    TooLarge {
        measured: Option<u64>,
        cap: u64,
    },
    /// So many files or directories that the state could not be measured against the cap.
    TooManyFiles {
        cap: u64,
    },
    /// The blueprint's filesystem, or a volume it names, cannot be opened.
    Unavailable(String),
    Io(String),
}

impl std::fmt::Display for ThrowawayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let advice = |cap: &u64| {
            format!(
                "over the {} MiB a test run copies; delete files or use a smaller volume",
                cap >> 20
            )
        };
        match self {
            Self::TooLarge {
                measured: Some(bytes),
                cap,
            } => {
                // Rounded up, so a state a byte over the cap does not read as equal to it.
                let tenths_of_a_mib = bytes.saturating_mul(10).div_ceil(1 << 20);
                write!(
                    f,
                    "the local state to copy is {}.{} MiB, {}",
                    tenths_of_a_mib / 10,
                    tenths_of_a_mib % 10,
                    advice(cap)
                )
            }
            Self::TooLarge {
                measured: None,
                cap,
            } => write!(f, "the local state to copy is {}", advice(cap)),
            Self::TooManyFiles { cap } => write!(
                f,
                "the local state to copy has too many files or directories to measure, so it \
                 may be {}",
                advice(cap)
            ),
            Self::Unavailable(message) => f.write_str(message),
            Self::Io(message) => write!(f, "copying local state failed: {message}"),
        }
    }
}

impl std::error::Error for ThrowawayError {}

/// The copies a test run runs against. Dropping it deletes them, off the dropping thread
/// when it is a runtime's.
pub struct Throwaway {
    /// Holds every copy; taken out to be deleted on drop.
    dir: Option<tempfile::TempDir>,
    /// The copy of the source session's directory, for a `per_session` blueprint.
    pub session_root: Option<PathBuf>,
    /// The volumes as the run resolves them: copies, and the originals of read-only ones.
    pub volumes: VolumeRegistry,
    pub kv: Arc<ForkedSessionKv>,
    pub report: LocalState,
}

impl Throwaway {
    /// Copies what a run of `blueprint` under `variables` would touch of `source_session`,
    /// or builds empty state when the session is gone.
    pub async fn copy(
        manager: &SessionManager,
        source_session: Option<&str>,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<Self, ThrowawayError> {
        Self::copy_capped(
            manager,
            source_session,
            blueprint,
            variables,
            LOCAL_STATE_CAP_BYTES,
        )
        .await
    }

    async fn copy_capped(
        manager: &SessionManager,
        source_session: Option<&str>,
        blueprint: &Blueprint,
        variables: &VarBindings,
        cap: u64,
    ) -> Result<Self, ThrowawayError> {
        let config = blueprint
            .vfs
            .resolve(variables)
            .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?;
        let per_session = matches!(config, VfsConfig::PerSession { .. });
        let source_root = if per_session {
            source_session.and_then(|session| manager.session_vfs_root(session))
        } else {
            None
        };
        let planned = plan_volumes(manager, &config)?;
        let session_found = source_session.is_some_and(|session| manager.contains(session));
        let kv_base = source_session
            .filter(|session| manager.contains(session))
            .map(|session| manager.session_kv_for_execute(session));
        let dir = tempfile::tempdir().map_err(|error| ThrowawayError::Io(error.to_string()))?;
        let to_copy = planned
            .iter()
            .filter(|volume| volume.copy)
            .map(|volume| (volume.name.clone(), volume.host.clone()))
            .collect();
        // The blocking task owns the directory until the copy is done, so a caller that
        // gives up meanwhile cannot have it deleted from under the copy.
        let (dir, copied) = tokio::task::spawn_blocking(move || {
            let copied = copy_all(dir.path(), source_root, to_copy, cap);
            copied.map(|copied| (dir, copied))
        })
        .await
        .map_err(|error| ThrowawayError::Io(error.to_string()))??;
        let volumes_copied = planned
            .iter()
            .filter(|volume| volume.copy)
            .map(|volume| volume.name.clone())
            .collect();
        // A workspace the source session no longer has starts empty.
        let session_root = per_session.then(|| copied.session.clone());
        let throwaway = Self {
            report: LocalState {
                as_of_now: true,
                session_found: session_found && !copied.session_gone,
                volumes_copied,
                bytes_copied: copied.bytes,
            },
            dir: Some(dir),
            session_root,
            volumes: VolumeRegistry::new(volume_table(planned, &copied.volumes), PathBuf::new()),
            kv: Arc::new(ForkedSessionKv::new(kv_base)),
        };
        // Built first, so a failure here deletes the copies off this worker too.
        if let Some(root) = &throwaway.session_root {
            std::fs::create_dir_all(root).map_err(|error| ThrowawayError::Io(error.to_string()))?;
        }
        Ok(throwaway)
    }
}

impl Drop for Throwaway {
    fn drop(&mut self) {
        let Some(dir) = self.dir.take() else {
            return;
        };
        // Deleting a tree is blocking work, kept off an async worker.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn_blocking(move || drop(dir));
            }
            Err(_) => drop(dir),
        }
    }
}

/// The volumes as the run resolves them: a copied one is its copy, any other the original,
/// which nothing can write to.
fn volume_table(planned: Vec<PlannedVolume>, copies: &BTreeMap<String, PathBuf>) -> VolumeTable {
    planned
        .into_iter()
        .map(|volume| {
            let (path, access) = match copies.get(&volume.name) {
                Some(copy) => (copy.clone(), volume.declared.access),
                None => (volume.host, Access::ReadOnly),
            };
            let spec = VolumeSpec {
                kind: VolumeKind::LocalPath { path },
                access,
                size_limit: volume.declared.size_limit,
            };
            (volume.name, spec)
        })
        .collect()
}

/// A named volume the blueprint references, and whether a run may write to it.
struct PlannedVolume {
    name: String,
    host: PathBuf,
    declared: VolumeSpec,
    /// Some reference to it resolves to read-write, so it is copied. Otherwise the run
    /// shares the original, read-only.
    copy: bool,
}

/// Every volume the blueprint references, once each, in order of first reference. A volume
/// is copied when any of its references may write: the first one alone does not say.
fn plan_volumes(
    manager: &SessionManager,
    config: &VfsConfig,
) -> Result<Vec<PlannedVolume>, ThrowawayError> {
    let mut planned: Vec<PlannedVolume> = Vec::new();
    for reference in config.named_references() {
        let resolved = manager
            .volume_registry()
            .resolve(reference.volume, reference.access)
            .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?;
        let writes = resolved.access == Access::ReadWrite;
        if let Some(volume) = planned.iter_mut().find(|v| v.name == reference.volume) {
            volume.copy |= writes;
            continue;
        }
        let Some(declared) = manager.volumes().get(reference.volume).cloned() else {
            return Err(ThrowawayError::Unavailable(format!(
                "volume '{}' is not declared on this server",
                reference.volume
            )));
        };
        planned.push(PlannedVolume {
            name: reference.volume.to_owned(),
            host: resolved.host,
            declared,
            copy: writes,
        });
    }
    Ok(planned)
}

/// One directory to copy: the source session's workspace (`volume` is `None`) or a volume.
struct CopyJob {
    volume: Option<String>,
    from: PathBuf,
    to: PathBuf,
}

impl CopyJob {
    /// What is copied, for a message.
    fn describe(&self) -> String {
        match &self.volume {
            Some(name) => format!("volume '{name}'"),
            None => "the session's files".to_owned(),
        }
    }
}

struct Copied {
    /// Where the session's workspace copy goes, whether or not there was one to copy.
    session: PathBuf,
    /// Where each copied volume's copy is, by volume name.
    volumes: BTreeMap<String, PathBuf>,
    bytes: u64,
    /// The source session's directory was gone when the copy came to it.
    session_gone: bool,
}

/// Measures every source, refuses the lot above `cap`, then copies each under `dir`, counting
/// as it goes so a source that grew since it was measured cannot pass the cap either.
///
/// A volume's copy goes under a generated name, never one built from the volume's name.
fn copy_all(
    dir: &Path,
    session_from: Option<PathBuf>,
    volumes: Vec<(String, PathBuf)>,
    cap: u64,
) -> Result<Copied, ThrowawayError> {
    let session = dir.join("session");
    let mut jobs: Vec<CopyJob> = session_from
        .into_iter()
        .map(|from| CopyJob {
            volume: None,
            from,
            to: session.clone(),
        })
        .collect();
    jobs.extend(
        volumes
            .into_iter()
            .enumerate()
            .map(|(index, (name, from))| CopyJob {
                volume: Some(name),
                from,
                to: dir.join("volumes").join(index.to_string()),
            }),
    );
    let mut session_gone = false;
    let mut measured = 0u64;
    let mut present = Vec::with_capacity(jobs.len());
    let throwaway = dir
        .canonicalize()
        .map_err(|error| ThrowawayError::Io(error.to_string()))?;
    for job in jobs {
        refuse_if_holding(&throwaway, &job)?;
        match measure_host_dir_skipping_vanished(&job.from) {
            Ok(size) => {
                measured = measured.saturating_add(size);
                present.push(job);
            }
            // The session was reaped since it was looked up: it starts empty. An entry
            // vanishing inside a tree is skipped by the walk, so this is the root.
            Err(error) if error.kind() == io::ErrorKind::NotFound && job.volume.is_none() => {
                session_gone = true;
            }
            Err(error) => return Err(refusal(walk_stop(error), cap)),
        }
    }
    if measured > cap {
        return Err(ThrowawayError::TooLarge {
            measured: Some(measured),
            cap,
        });
    }
    let mut bytes = 0u64;
    let mut volumes = BTreeMap::new();
    for job in present {
        let before = bytes;
        match clone_tree(&job.from, &job.to, cap, &mut bytes) {
            Ok(()) => {
                if let Some(name) = job.volume {
                    volumes.insert(name, job.to);
                }
            }
            Err(CopyDirError::Io(error))
                if error.kind() == io::ErrorKind::NotFound && job.volume.is_none() =>
            {
                // Clears whatever of the copy was made before the session went.
                let _ = std::fs::remove_dir_all(&job.to);
                bytes = before;
                session_gone = true;
            }
            Err(stop) => return Err(refusal(stop, cap)),
        }
    }
    Ok(Copied {
        session,
        volumes,
        bytes,
        session_gone,
    })
}

/// Refuses a source that holds the directory the copies are made in: copying a tree into
/// itself would never end, and only the run's own files would fill it. A source that is
/// gone is left to its measure, which settles a reaped session and refuses a volume.
fn refuse_if_holding(throwaway: &Path, job: &CopyJob) -> Result<(), ThrowawayError> {
    let source = match job.from.canonicalize() {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ThrowawayError::Io(error.to_string())),
    };
    if throwaway.starts_with(&source) {
        return Err(ThrowawayError::Unavailable(format!(
            "{} cannot be copied for a test run: it holds the directory the copies are made in",
            job.describe()
        )));
    }
    Ok(())
}

/// What a stop while copying means to the caller.
fn refusal(stop: CopyDirError, cap: u64) -> ThrowawayError {
    match stop {
        CopyDirError::Io(error) => ThrowawayError::Io(error.to_string()),
        CopyDirError::OverCap => ThrowawayError::TooLarge {
            measured: None,
            cap,
        },
        CopyDirError::TooMany => ThrowawayError::TooManyFiles { cap },
    }
}

/// A failed measure or walk of a tree: too many entries is its own refusal.
fn walk_stop(error: io::Error) -> CopyDirError {
    if error.kind() == io::ErrorKind::InvalidData {
        CopyDirError::TooMany
    } else {
        CopyDirError::Io(error)
    }
}

/// Copies the directory `from` to the new path `to`, adding the bytes copied to `bytes`: a
/// clone where the filesystem offers one (APFS `clonefile`), else a copy that follows no
/// link. Stops once `bytes` passes `cap`.
fn clone_tree(from: &Path, to: &Path, cap: u64, bytes: &mut u64) -> Result<(), CopyDirError> {
    #[cfg(target_os = "macos")]
    if to
        .parent()
        .is_none_or(|parent| std::fs::create_dir_all(parent).is_ok())
        && clonefile(from, to).is_ok()
    {
        // The clone keeps the source's modes, and a read-only directory could not be
        // deleted with the rest of the throwaway, so it is made writable before anything
        // else can stop the copy.
        make_dirs_writable(to, 0).map_err(walk_stop)?;
        // A clone is one call, so what it took is measured after.
        *bytes = bytes.saturating_add(measure_host_dir_skipping_vanished(to).map_err(walk_stop)?);
        if *bytes > cap {
            return Err(CopyDirError::OverCap);
        }
        return Ok(());
    }
    copy_host_dir(from, to, cap, bytes)
}

#[cfg(target_os = "macos")]
fn clonefile(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path holds a NUL"))
    };
    let (from, to) = (c_path(from)?, c_path(to)?);
    // SAFETY: both are valid NUL-terminated strings that outlive the call.
    if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Lets the owner write to every directory under `root`, which is no link, going on past
/// one it cannot change and reporting the first error at the end. Nesting past the depth a
/// walk goes to is `InvalidData`, as for a measure.
#[cfg(target_os = "macos")]
fn make_dirs_writable(root: &Path, depth: usize) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if depth > MAX_MEASURED_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "directories nested too deep",
        ));
    }
    // Best effort: one directory that cannot be changed does not leave the rest read-only.
    let mut first = None;
    let mut note = |result: io::Result<()>| {
        if let Err(error) = result {
            first.get_or_insert(error);
        }
    };
    note(std::fs::symlink_metadata(root).and_then(|meta| {
        let mode = meta.permissions().mode();
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(mode | 0o700))
    }));
    match std::fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries {
                note(entry.and_then(|entry| {
                    if entry.file_type()?.is_dir() {
                        make_dirs_writable(&entry.path(), depth + 1)?;
                    }
                    Ok(())
                }));
            }
        }
        Err(error) => note(Err(error)),
    }
    first.map_or(Ok(()), Err)
}

/// A session's `submilli:session` data as a test run sees it: reads fall through to the
/// source session's store, and the run's own writes and removals stay here.
///
/// The run's own entries count against the store's limits; the source's do not.
pub struct ForkedSessionKv {
    base: Option<Arc<dyn SessionKvStore>>,
    own: InMemorySessionKv,
    /// Keys the run removed that the source still holds. Held across each operation, so
    /// the two stores change together.
    removed: Mutex<BTreeSet<Vec<u16>>>,
}

impl ForkedSessionKv {
    pub fn new(base: Option<Arc<dyn SessionKvStore>>) -> Self {
        let limits = base
            .as_ref()
            .map_or_else(SessionKvLimits::default, |base| base.limits());
        Self {
            base,
            own: InMemorySessionKv::new(limits),
            removed: Mutex::new(BTreeSet::new()),
        }
    }

    fn removed(&self) -> MutexGuard<'_, BTreeSet<Vec<u16>>> {
        self.removed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionKvStore for ForkedSessionKv {
    fn get(&self, key: &[u16]) -> Result<Option<Vec<u16>>, SessionKvError> {
        let removed = self.removed();
        if let Some(own) = self.own.get(key)? {
            return Ok(Some(own));
        }
        match &self.base {
            Some(base) if !removed.contains(key) => base.get(key),
            _ => Ok(None),
        }
    }

    fn has(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        let removed = self.removed();
        if self.own.has(key)? {
            return Ok(true);
        }
        match &self.base {
            Some(base) if !removed.contains(key) => base.has(key),
            _ => Ok(false),
        }
    }

    fn set(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError> {
        let mut removed = self.removed();
        self.own.set(key, payload)?;
        removed.remove(key);
        Ok(())
    }

    fn remove(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        let mut removed = self.removed();
        let was_own = self.own.remove(key)?;
        let in_base = match &self.base {
            Some(base) if !removed.contains(key) => base.has(key)?,
            _ => false,
        };
        if in_base {
            removed.insert(key.to_vec());
        }
        Ok(was_own || in_base)
    }

    /// Both stores are scanned and merged: the window ends where the earlier of the two
    /// stopped, so a page covers keys neither skipped. A page may look at up to twice
    /// `max_scan` keys.
    fn scan(
        &self,
        after: Option<&[u16]>,
        prefix: &[u16],
        max_scan: usize,
    ) -> Result<SessionKvPage, SessionKvError> {
        let removed = self.removed();
        let own = self.own.scan(after, prefix, max_scan)?;
        let Some(base) = &self.base else {
            return Ok(own);
        };
        let base = base.scan(after, prefix, max_scan)?;
        let end = [&own, &base]
            .into_iter()
            .filter(|page| !page.scanned_all)
            .filter_map(|page| page.last_scanned.clone())
            .min();
        let within = |key: &Vec<u16>| end.as_ref().is_none_or(|end| key <= end);
        let mut merged: BTreeMap<Vec<u16>, u64> = BTreeMap::new();
        for entry in base.entries {
            if within(&entry.key) && !removed.contains(&entry.key) {
                merged.insert(entry.key, entry.size_bytes);
            }
        }
        for entry in own.entries {
            if within(&entry.key) {
                merged.insert(entry.key, entry.size_bytes);
            }
        }
        let scanned_all = own.scanned_all && base.scanned_all;
        Ok(SessionKvPage {
            entries: merged
                .into_iter()
                .map(|(key, size_bytes)| SessionKvEntry { key, size_bytes })
                .collect(),
            last_scanned: if scanned_all {
                own.last_scanned.max(base.last_scanned)
            } else {
                end
            },
            scanned_all,
        })
    }

    fn limits(&self) -> SessionKvLimits {
        self.own.limits()
    }
}

#[cfg(test)]
mod tests {
    use submilli_blueprint::HarnessSecretBindings;

    use super::*;
    use crate::config::SizeLimit;
    use crate::idempotency_store::InMemoryIdempotencyStore;
    use crate::session_manager::CapabilitySettings;
    use crate::session_store::InMemoryDurableSessionStore;

    fn key(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn source_with(entries: &[(&str, &str)]) -> Arc<InMemorySessionKv> {
        let source = Arc::new(InMemorySessionKv::default());
        for (name, value) in entries {
            source.set(&key(name), &key(value)).expect("set");
        }
        source
    }

    fn scanned(store: &dyn SessionKvStore, prefix: &str) -> Vec<String> {
        let page = store.scan(None, &key(prefix), 100).expect("scan");
        assert!(page.scanned_all);
        page.entries
            .iter()
            .map(|entry| String::from_utf16_lossy(&entry.key))
            .collect()
    }

    #[test]
    fn a_forked_store_reads_through_and_keeps_its_own_writes() {
        let source = source_with(&[("a", "1"), ("b", "2")]);
        let forked = ForkedSessionKv::new(Some(source.clone()));
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("1")));
        forked.set(&key("a"), &key("changed")).unwrap();
        forked.set(&key("c"), &key("3")).unwrap();
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("changed")));
        assert!(forked.has(&key("c")).unwrap());
        assert_eq!(source.get(&key("a")).unwrap(), Some(key("1")));
        assert!(!source.has(&key("c")).unwrap());
    }

    #[test]
    fn a_removal_hides_the_source_entry_until_it_is_set_again() {
        let source = source_with(&[("a", "1")]);
        let forked = ForkedSessionKv::new(Some(source.clone()));
        assert!(forked.remove(&key("a")).unwrap());
        assert!(!forked.remove(&key("a")).unwrap(), "already removed");
        assert_eq!(forked.get(&key("a")).unwrap(), None);
        assert!(!forked.has(&key("a")).unwrap());
        assert!(scanned(&forked, "").is_empty());
        assert!(source.has(&key("a")).unwrap(), "the source keeps it");
        forked.set(&key("a"), &key("back")).unwrap();
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("back")));
        assert_eq!(scanned(&forked, ""), ["a"]);
    }

    #[test]
    fn a_scan_merges_both_stores_in_key_order() {
        let source = source_with(&[("a", "1"), ("c", "3"), ("x1", "9")]);
        let forked = ForkedSessionKv::new(Some(source));
        forked.set(&key("b"), &key("2")).unwrap();
        forked.set(&key("c"), &key("override")).unwrap();
        forked.remove(&key("a")).unwrap();
        assert_eq!(scanned(&forked, ""), ["b", "c", "x1"]);
        assert_eq!(scanned(&forked, "x"), ["x1"]);
        let page = forked.scan(None, &key(""), 100).unwrap();
        let sizes: Vec<u64> = page.entries.iter().map(|entry| entry.size_bytes).collect();
        assert_eq!(
            sizes[1],
            2 * "override".len() as u64,
            "the run's value wins"
        );
    }

    #[test]
    fn a_bounded_scan_pages_through_the_merge() {
        let source = source_with(&[("a", "1"), ("c", "3"), ("e", "5")]);
        let forked = ForkedSessionKv::new(Some(source));
        forked.set(&key("b"), &key("2")).unwrap();
        forked.set(&key("d"), &key("4")).unwrap();
        let mut seen = Vec::new();
        let mut after: Option<Vec<u16>> = None;
        loop {
            let page = forked.scan(after.as_deref(), &key(""), 2).unwrap();
            seen.extend(
                page.entries
                    .iter()
                    .map(|entry| String::from_utf16_lossy(&entry.key)),
            );
            if page.scanned_all {
                break;
            }
            after = page.last_scanned;
        }
        assert_eq!(seen, ["a", "b", "c", "d", "e"]);
    }

    fn manager(volumes: VolumeTable, root: &Path) -> SessionManager {
        SessionManager::new(
            root.to_path_buf(),
            None,
            Arc::new(VolumeRegistry::new(volumes, root.join("managed"))),
            Arc::new(|| -> Arc<dyn interpreter::stdlib::http::HttpClient> {
                panic!("no HTTP in this test")
            }),
            Arc::new(InMemoryDurableSessionStore::default()),
            Arc::new(InMemoryIdempotencyStore::default()),
            CapabilitySettings::default(),
        )
    }

    fn blueprint() -> Blueprint {
        submilli_blueprint::parse(
            "name: x\nidle_timeout: 1h\nvfs:\n  mode: per_session\n  mounts:\n    /data: {mode: named, volume: data}\n    /ref: {mode: named, volume: reference, access: read_only}\n",
        )
        .expect("blueprint")
    }

    fn volumes(data: &Path, reference: &Path) -> VolumeTable {
        VolumeTable::from([
            ("data".to_owned(), VolumeSpec::local_path(data)),
            (
                "reference".to_owned(),
                VolumeSpec {
                    access: Access::ReadOnly,
                    size_limit: SizeLimit::Unlimited,
                    ..VolumeSpec::local_path(reference)
                },
            ),
        ])
    }

    #[tokio::test]
    async fn a_copy_holds_what_is_there_now_and_writes_to_it_leave_the_source_alone() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("log.txt"), "one\n").unwrap();
        std::fs::write(reference.join("ro.txt"), "fixed").unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let blueprint = blueprint();
        let session = manager
            .create(
                &blueprint,
                Arc::new(VarBindings::new()),
                Arc::new(HarnessSecretBindings::default()),
            )
            .await
            .unwrap();
        let session_dir = manager.session_vfs_root(&session).expect("per_session dir");
        std::fs::write(session_dir.join("notes.txt"), "written by the source run").unwrap();
        manager
            .session_kv_for_execute(&session)
            .set(&key("k"), &key("v"))
            .unwrap();

        let copy = Throwaway::copy(&manager, Some(&session), &blueprint, &VarBindings::new())
            .await
            .expect("copy");
        assert!(copy.report.as_of_now && copy.report.session_found);
        assert_eq!(copy.report.volumes_copied, ["data"]);
        let copied_session = copy.session_root.clone().expect("session copy");
        assert_eq!(
            std::fs::read_to_string(copied_session.join("notes.txt")).unwrap(),
            "written by the source run"
        );
        assert_eq!(copy.kv.get(&key("k")).unwrap(), Some(key("v")));

        // The run's volume resolves to the copy, the read-only one to the original.
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_ne!(in_copy, data);
        assert_eq!(
            copy.volumes.resolve("reference", None).unwrap().host,
            reference
        );
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(in_copy.join("log.txt"))
            .unwrap();
        std::io::Write::write_all(&mut log, b"two\n").unwrap();
        std::fs::write(copied_session.join("notes.txt"), "changed").unwrap();
        copy.kv.set(&key("k"), &key("changed")).unwrap();

        assert_eq!(std::fs::read(data.join("log.txt")).unwrap(), b"one\n");
        assert_eq!(
            std::fs::read_to_string(session_dir.join("notes.txt")).unwrap(),
            "written by the source run"
        );
        assert_eq!(
            manager
                .session_kv_for_execute(&session)
                .get(&key("k"))
                .unwrap(),
            Some(key("v"))
        );

        let held = copy.session_root.clone().unwrap();
        drop(copy);
        for _ in 0..200 {
            if !held.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!held.exists(), "the copies go with the throwaway");
    }

    #[tokio::test]
    async fn a_session_that_is_gone_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let copy = Throwaway::copy(&manager, Some("gone"), &blueprint(), &VarBindings::new())
            .await
            .expect("copy");
        assert!(!copy.report.session_found);
        let session = copy.session_root.as_ref().expect("an empty workspace");
        assert_eq!(std::fs::read_dir(session).unwrap().count(), 0);
        assert!(!copy.kv.has(&key("k")).unwrap());
    }

    #[tokio::test]
    async fn local_state_over_the_cap_is_refused_with_the_cap_in_the_message() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("big.bin"), vec![0u8; 3 << 20]).unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let error =
            Throwaway::copy_capped(&manager, None, &blueprint(), &VarBindings::new(), 2 << 20)
                .await
                .err()
                .expect("refused");
        assert!(matches!(error, ThrowawayError::TooLarge { .. }), "{error}");
        let message = error.to_string();
        assert!(
            message.contains("3.0 MiB") && message.contains("2 MiB"),
            "{message}"
        );
    }

    #[test]
    fn a_state_a_byte_over_the_cap_does_not_read_as_equal_to_it() {
        let cap = 256 << 20;
        let over = ThrowawayError::TooLarge {
            measured: Some(cap + 1),
            cap,
        };
        let message = over.to_string();
        assert!(
            message.contains("256.1 MiB") && message.contains("256 MiB a test"),
            "{message}"
        );
        let grew = ThrowawayError::TooLarge {
            measured: None,
            cap,
        };
        let message = grew.to_string();
        assert!(!message.contains("0 MiB, "), "no partial size: {message}");
    }

    #[test]
    fn a_volume_named_like_a_path_is_copied_inside_the_throwaway_only() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("f"), "x").unwrap();
        let dir = root.path().join("throwaway");
        std::fs::create_dir_all(&dir).unwrap();
        let names = [
            "../../escape".to_owned(),
            "a/b".to_owned(),
            "..".to_owned(),
            source.to_string_lossy().into_owned(),
        ];
        let volumes = names
            .iter()
            .map(|name| (name.clone(), source.clone()))
            .collect();
        let copied = copy_all(&dir, None, volumes, 1 << 20).expect("copied");
        assert_eq!(copied.volumes.len(), names.len());
        for (name, copy) in &copied.volumes {
            assert!(copy.starts_with(&dir), "{name} copied to {copy:?}");
            assert_eq!(std::fs::read_to_string(copy.join("f")).unwrap(), "x");
        }
        assert_eq!(std::fs::read_to_string(source.join("f")).unwrap(), "x");
        assert!(!root.path().join("escape").exists());
        assert_eq!(std::fs::read_dir(&source).unwrap().count(), 1);
    }

    #[test]
    fn a_volume_that_is_missing_is_an_error_and_only_the_session_root_means_reaped() {
        let root = tempfile::tempdir().unwrap();
        let error = copy_all(
            root.path(),
            None,
            vec![("v".to_owned(), root.path().join("absent"))],
            1 << 20,
        )
        .err()
        .expect("a missing volume is not a reaped session");
        assert!(matches!(error, ThrowawayError::Io(_)), "{error}");
    }

    fn volume_blueprint(mounts: &str) -> Blueprint {
        submilli_blueprint::parse(&format!(
            "name: x\nidle_timeout: 1h\nvfs:\n  mode: per_session\n  mounts:\n{mounts}"
        ))
        .expect("blueprint")
    }

    async fn copied(blueprint: &Blueprint) -> (tempfile::TempDir, PathBuf, Throwaway) {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("log.txt"), "one\n").unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let copy = Throwaway::copy(&manager, None, blueprint, &VarBindings::new())
            .await
            .expect("copy");
        (root, data, copy)
    }

    #[tokio::test]
    async fn dropping_a_throwaway_deletes_its_directory_off_the_worker() {
        let (_root, _data, copy) = copied(&blueprint()).await;
        let held = copy
            .session_root
            .as_ref()
            .and_then(|root| root.parent())
            .expect("a directory")
            .to_owned();
        assert!(held.exists());
        drop(copy);
        for _ in 0..200 {
            if !held.exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the throwaway directory was not deleted");
    }

    #[test]
    fn dropping_a_throwaway_outside_a_runtime_deletes_it_inline() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (_root, _data, copy) = runtime.block_on(copied(&blueprint()));
        let held = copy
            .session_root
            .as_ref()
            .and_then(|root| root.parent())
            .expect("a directory")
            .to_owned();
        drop(copy);
        assert!(!held.exists());
    }

    #[test]
    fn a_volume_holding_the_throwaway_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("inner/throwaway");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.path().join("f"), "x").unwrap();
        let error = copy_all(
            &dir,
            None,
            vec![("v".to_owned(), root.path().to_owned())],
            1 << 20,
        )
        .err()
        .expect("refused");
        let message = error.to_string();
        assert!(matches!(error, ThrowawayError::Unavailable(_)), "{message}");
        assert!(message.contains("volume 'v'"), "{message}");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            0,
            "nothing copied"
        );
    }

    #[test]
    fn a_session_directory_holding_the_throwaway_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("inner/throwaway");
        std::fs::create_dir_all(&dir).unwrap();
        let error = copy_all(&dir, Some(root.path().to_owned()), Vec::new(), 1 << 20)
            .err()
            .expect("refused");
        let message = error.to_string();
        assert!(matches!(error, ThrowawayError::Unavailable(_)), "{message}");
        assert!(message.contains("the session's files"), "{message}");
    }

    #[test]
    fn a_source_that_cannot_be_resolved_for_the_check_is_an_error_not_a_pass() {
        let root = tempfile::tempdir().unwrap();
        // A path through a file: neither found nor not found, so it cannot be told apart.
        std::fs::write(root.path().join("file"), "x").unwrap();
        let error = copy_all(
            root.path(),
            Some(root.path().join("file/session")),
            Vec::new(),
            1 << 20,
        )
        .err()
        .expect("refused");
        assert!(matches!(error, ThrowawayError::Io(_)), "{error}");
    }

    #[tokio::test]
    async fn a_volume_any_reference_may_write_is_copied_whatever_order_they_come_in() {
        // `/a` reads and `/b` writes the same volume.
        let blueprint = volume_blueprint(
            "    /a: {mode: named, volume: data, access: read_only}\n    /b: {mode: named, volume: data, access: read_write}\n",
        );
        let (_root, data, copy) = copied(&blueprint).await;
        assert_eq!(copy.report.volumes_copied, ["data"]);
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_ne!(in_copy, data);
        std::fs::write(in_copy.join("log.txt"), "changed").unwrap();
        assert_eq!(std::fs::read(data.join("log.txt")).unwrap(), b"one\n");
    }

    #[tokio::test]
    async fn a_volume_only_read_is_shared_and_registered_read_only() {
        // The server declares `data` read-write; the blueprint only reads it.
        let blueprint =
            volume_blueprint("    /a: {mode: named, volume: data, access: read_only}\n");
        let (_root, data, copy) = copied(&blueprint).await;
        assert!(copy.report.volumes_copied.is_empty());
        let shared = copy
            .volumes
            .resolve("data", Some(Access::ReadWrite))
            .unwrap();
        assert_eq!(shared.host, data);
        assert_eq!(
            shared.access,
            Access::ReadOnly,
            "nothing can write the original"
        );
        assert_eq!(copy.volumes.table()["data"].access, Access::ReadOnly);
    }

    #[tokio::test]
    async fn a_read_only_root_and_a_writable_mount_of_one_volume_are_copied() {
        let blueprint = submilli_blueprint::parse(
            "name: x\nvfs:\n  mode: named\n  volume: data\n  access: read_only\n  mounts:\n    /m: {mode: named, volume: data}\n",
        )
        .expect("blueprint");
        let (_root, data, copy) = copied(&blueprint).await;
        assert_eq!(copy.report.volumes_copied, ["data"]);
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_ne!(in_copy, data);
    }

    #[test]
    fn a_copy_stops_once_the_bytes_copied_pass_the_cap() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(from.join("sub")).unwrap();
        std::fs::write(from.join("a.bin"), vec![0u8; 600]).unwrap();
        std::fs::write(from.join("sub/b.bin"), vec![0u8; 600]).unwrap();
        let mut bytes = 0;
        let stop = clone_tree(&from, &root.path().join("to"), 1000, &mut bytes);
        assert!(matches!(stop, Err(CopyDirError::OverCap)));
        let mut bytes = 0;
        clone_tree(&from, &root.path().join("fits"), 1200, &mut bytes)
            .unwrap_or_else(|_| panic!("fits"));
        assert_eq!(bytes, 1200);
    }

    #[test]
    fn too_many_entries_is_its_own_refusal() {
        let error = std::io::Error::new(io::ErrorKind::InvalidData, "more than 3 entries");
        let refused = refusal(walk_stop(error), 5 << 20);
        assert!(matches!(refused, ThrowawayError::TooManyFiles { cap } if cap == 5 << 20));
        let message = refused.to_string();
        assert!(
            message.contains("too many files") && message.contains("5 MiB"),
            "{message}"
        );
        let other = refusal(walk_stop(io::Error::other("disk")), 1);
        assert!(matches!(other, ThrowawayError::Io(_)));
    }

    #[test]
    fn a_session_directory_gone_before_the_copy_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        let copied = copy_all(
            root.path(),
            Some(root.path().join("reaped")),
            Vec::new(),
            1 << 20,
        )
        .expect("a reaped session is not an error");
        assert!(copied.session_gone && copied.volumes.is_empty());
        assert!(!copied.session.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_session_is_copied_as_a_link_and_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(&from).unwrap();
        std::os::unix::fs::symlink(&outside, from.join("link")).unwrap();
        std::os::unix::fs::symlink("missing", from.join("dangling")).unwrap();
        let to = root.path().join("to");
        clone_tree(&from, &to, 1 << 20, &mut 0).unwrap_or_else(|_| panic!("copied"));
        assert_eq!(std::fs::read_link(to.join("link")).unwrap(), outside);
        assert_eq!(
            std::fs::read_link(to.join("dangling")).unwrap(),
            Path::new("missing")
        );
        assert!(
            std::fs::symlink_metadata(to.join("link"))
                .unwrap()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_of_a_read_only_tree_can_be_deleted() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(from.join("locked")).unwrap();
        std::fs::write(from.join("locked/file"), "x").unwrap();
        std::fs::set_permissions(from.join("locked"), std::fs::Permissions::from_mode(0o555))
            .unwrap();
        let kept = tempfile::tempdir().unwrap();
        let to = kept.path().join("to");
        let outcome = clone_tree(&from, &to, 1 << 20, &mut 0);
        std::fs::set_permissions(from.join("locked"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(outcome.is_ok());
        let held = kept.path().to_owned();
        drop(kept);
        assert!(!held.exists(), "the throwaway directory is deleted whole");
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_tree_over_the_cap_can_still_be_deleted() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(from.join("locked")).unwrap();
        std::fs::write(from.join("locked/file"), "x").unwrap();
        std::fs::set_permissions(from.join("locked"), std::fs::Permissions::from_mode(0o555))
            .unwrap();
        let kept = tempfile::tempdir().unwrap();
        let outcome = clone_tree(&from, &kept.path().join("to"), 0, &mut 0);
        std::fs::set_permissions(from.join("locked"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(matches!(outcome, Err(CopyDirError::OverCap)));
        let held = kept.path().to_owned();
        drop(kept);
        assert!(!held.exists(), "the throwaway directory is deleted whole");
    }
}
