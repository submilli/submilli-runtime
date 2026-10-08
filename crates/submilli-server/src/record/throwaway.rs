//! Throwaway local state for a test run: copies of the files and session data the run
//! would touch, so it runs live against them and leaves the originals alone.
//!
//! The source session's `per_session` directory and each writable named volume the
//! blueprint mounts are copied into a temporary directory (a clone where the filesystem
//! has one, else a plain copy), up to [`LOCAL_STATE_CAP_BYTES`]. Of a volume only the
//! sub-paths the blueprint mounts are copied. The session's `submilli:session`
//! data is read through [`ForkedSessionKv`]. Everything goes when the [`Throwaway`] drops.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(target_os = "macos")]
use interpreter::runtime::MAX_MEASURED_DEPTH;
#[cfg(target_os = "macos")]
use interpreter::runtime::open_host_subdir;
use interpreter::runtime::session_kv::{
    InMemorySessionKv, SessionKvEntry, SessionKvError, SessionKvLimits, SessionKvPage,
    SessionKvStore,
};
use interpreter::runtime::{
    CopyDirError, copy_host_dir, copy_host_subdir, measure_host_dir_skipping_vanished,
    measure_host_subdir_skipping_vanished,
};
use serde::Serialize;
use submilli_blueprint::{Blueprint, VarBindings, VfsConfig};

use crate::config::{Access, SizeLimit, VolumeKind, VolumeSpec, VolumeTable};
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
            match source_session {
                Some(session) => manager
                    .session_vfs_root(session)
                    .await
                    .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?,
                None => None,
            }
        } else {
            None
        };
        let planned = plan_volumes(manager, &config)?;
        let session_found = match source_session {
            Some(session) => manager
                .contains(session)
                .await
                .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?,
            None => false,
        };
        let kv_base = source_session
            .filter(|_| session_found)
            .map(|session| manager.session_kv_for_execute(session));
        let dir = tempfile::tempdir().map_err(|error| ThrowawayError::Io(error.to_string()))?;
        let to_copy = planned
            .iter()
            .filter(|volume| volume.copy)
            .map(|volume| {
                (
                    volume.name.clone(),
                    volume.host.clone(),
                    outermost(&volume.reach),
                )
            })
            .collect();
        let limited: Vec<(String, PathBuf)> = planned
            .iter()
            .filter(|volume| {
                volume.copy && matches!(volume.declared.size_limit, SizeLimit::Bytes(_))
            })
            .map(|volume| (volume.name.clone(), volume.host.clone()))
            .collect();
        // The blocking task owns the directory until the copy is done, so a caller that
        // gives up meanwhile cannot have it deleted from under the copy.
        let (dir, copied, usage_offsets) = tokio::task::spawn_blocking(move || {
            let copied = copy_all(dir.path(), source_root, to_copy, cap)?;
            let offsets = usage_offsets(&limited, &copied);
            Ok::<_, ThrowawayError>((dir, copied, offsets))
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
            volumes: VolumeRegistry::new(volume_table(planned, &copied.volumes), PathBuf::new())
                .with_usage_offsets(usage_offsets),
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

/// For each volume with a size limit, what the copy leaves out of its usage: the source's
/// size now less what was copied, so the run's quota counts the whole volume. `None` when
/// the source cannot be measured, which the quota treats as full.
fn usage_offsets(limited: &[(String, PathBuf)], copied: &Copied) -> BTreeMap<String, Option<u64>> {
    limited
        .iter()
        .map(|(name, host)| {
            let left_out = measure_host_dir_skipping_vanished(host).ok().map(|used| {
                let taken = copied.volume_bytes.get(name).copied().unwrap_or(0);
                used.saturating_sub(taken)
            });
            (name.clone(), left_out)
        })
        .collect()
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
    /// The directory each reference is limited to, relative to the volume; empty for the
    /// whole volume.
    reach: Vec<PathBuf>,
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
        let reach = relative_path(reference.sub_path)?;
        if let Some(volume) = planned.iter_mut().find(|v| v.name == reference.volume) {
            volume.copy |= writes;
            volume.reach.push(reach);
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
            reach: vec![reach],
        });
    }
    Ok(planned)
}

/// A resolved sub-path as a path relative to its volume, empty for none. One that is not
/// a plain relative path is refused: the blueprint's resolution never makes one, so this
/// holds the copy to the volume whatever reaches it.
fn relative_path(sub_path: Option<&str>) -> Result<PathBuf, ThrowawayError> {
    let Some(path) = sub_path else {
        return Ok(PathBuf::new());
    };
    if path.is_empty()
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.contains(['\\', '\0'])
        })
    {
        return Err(ThrowawayError::Unavailable(format!(
            "the volume sub-path '{path}' is not a normalized relative path"
        )));
    }
    Ok(PathBuf::from(path))
}

/// `paths` without any that lies inside another, by path components, in order. An empty
/// path is the whole volume and leaves only itself. On a case-insensitive filesystem,
/// paths differing only by case are kept apart: each is copied and counted, so the copy is
/// right but a byte may count twice against the cap.
fn outermost(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut sorted = paths.to_vec();
    // A path sorts before everything inside it.
    sorted.sort();
    sorted.dedup();
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in sorted {
        if !kept.iter().any(|outer| path.starts_with(outer)) {
            kept.push(path);
        }
    }
    kept
}

/// One directory to copy: the source session's workspace (`volume` is `None`) or the
/// directory `sub` of a volume, which goes to `sub` under `to`.
struct CopyJob {
    volume: Option<String>,
    from: PathBuf,
    sub: PathBuf,
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
    /// The bytes copied of each volume, by name.
    volume_bytes: BTreeMap<String, u64>,
    /// The source session's directory was gone when the copy came to it.
    session_gone: bool,
}

/// Measures every source, refuses the lot above `cap`, then copies each under `dir`, counting
/// as it goes so a source that grew since it was measured cannot pass the cap either.
///
/// A volume's copy goes under a generated name, never one built from the volume's name. Each
/// volume comes with the sub-paths of it to copy, none inside another; an empty one is all
/// of it.
fn copy_all(
    dir: &Path,
    session_from: Option<PathBuf>,
    volumes: Vec<(String, PathBuf, Vec<PathBuf>)>,
    cap: u64,
) -> Result<Copied, ThrowawayError> {
    let session = dir.join("session");
    let mut jobs: Vec<CopyJob> = session_from
        .into_iter()
        .map(|from| CopyJob {
            volume: None,
            from,
            sub: PathBuf::new(),
            to: session.clone(),
        })
        .collect();
    for (index, (name, from, subs)) in volumes.into_iter().enumerate() {
        let to = dir.join("volumes").join(index.to_string());
        jobs.extend(subs.into_iter().map(|sub| CopyJob {
            volume: Some(name.clone()),
            from: from.clone(),
            sub,
            to: to.clone(),
        }));
    }
    let mut session_gone = false;
    let mut measured = 0u64;
    let mut present = Vec::with_capacity(jobs.len());
    let throwaway = dir
        .canonicalize()
        .map_err(|error| ThrowawayError::Io(error.to_string()))?;
    for job in jobs {
        refuse_if_holding(&throwaway, &job)?;
        match measure_host_subdir_skipping_vanished(&job.from, &job.sub) {
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
    let mut volume_bytes: BTreeMap<String, u64> = BTreeMap::new();
    for job in present {
        let before = bytes;
        match clone_subtree(&job.from, &job.sub, &job.to, cap, &mut bytes) {
            Ok(()) => {
                if let Some(name) = job.volume {
                    *volume_bytes.entry(name.clone()).or_default() += bytes - before;
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
        volume_bytes,
        session_gone,
    })
}

/// Refuses a source that holds the directory the copies are made in: copying a tree into
/// itself would never end, and only the run's own files would fill it. A source that is
/// gone is left to its measure: a reaped session starts empty, a missing sub-path of a
/// volume measures 0 and copies nothing, and only a missing volume root is refused.
fn refuse_if_holding(throwaway: &Path, job: &CopyJob) -> Result<(), ThrowawayError> {
    let source = match job.from.join(&job.sub).canonicalize() {
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
    if try_clone(to, cap, bytes, || clonefile(from, to))? {
        return Ok(());
    }
    copy_host_dir(from, to, cap, bytes)
}

/// As [`clone_tree`] for the directory `sub` of the volume `from`, which lands at `sub`
/// under `to`. `sub` is walked without following links first, so one that is a link, or
/// passes through one, is refused; one that is not there copies nothing. An empty `sub`
/// is the whole of `from`.
fn clone_subtree(
    from: &Path,
    sub: &Path,
    to: &Path,
    cap: u64,
    bytes: &mut u64,
) -> Result<(), CopyDirError> {
    if sub.as_os_str().is_empty() {
        return clone_tree(from, to, cap, bytes);
    }
    #[cfg(target_os = "macos")]
    if clone_subdir(from, sub, to, cap, bytes)? {
        return Ok(());
    }
    copy_host_subdir(from, sub, to, cap, bytes)
}

/// Clones `sub` of the volume `from` to `to/sub`; `false` when a copy is needed. The clone
/// is anchored to the opened, link-free parent of `sub`, so a link swapped in above `sub`
/// after the walk is not followed, and one swapped in for `sub` itself is cloned as a link
/// and removed. A `sub` that is a link is refused, one that is not there copies nothing.
#[cfg(target_os = "macos")]
fn clone_subdir(
    from: &Path,
    sub: &Path,
    to: &Path,
    cap: u64,
    bytes: &mut u64,
) -> Result<bool, CopyDirError> {
    use std::os::fd::AsRawFd;

    let (Some(name), Some(parent_sub)) = (sub.file_name(), sub.parent()) else {
        return Ok(false);
    };
    let Some(parent) = open_host_subdir(from, parent_sub)? else {
        std::fs::create_dir_all(to)?;
        return Ok(true);
    };
    let c_name = c_string(name)?;
    // SAFETY: an all-zero `stat` is a valid out-parameter, and `c_name` is NUL-terminated.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            c_name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            return Err(sub_error(sub, error).into());
        }
        std::fs::create_dir_all(to)?;
        return Ok(true);
    }
    match stat.st_mode & libc::S_IFMT {
        libc::S_IFDIR => {}
        libc::S_IFLNK => {
            return Err(sub_error(sub, io::Error::from_raw_os_error(libc::ELOOP)).into());
        }
        _ => return Err(sub_error(sub, io::Error::from_raw_os_error(libc::ENOTDIR)).into()),
    }
    let target = to.join(sub);
    try_clone(&target, cap, bytes, || {
        clone_entry(parent.as_raw_fd(), &c_name, &target)
    })
}

/// Clones the entry `name` of the directory open as `parent` to the new path `target`. An
/// entry that is a link by then is cloned as one, so it is removed and the clone fails.
#[cfg(target_os = "macos")]
fn clone_entry(parent: std::os::fd::RawFd, name: &std::ffi::CStr, target: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let dst_dir = std::fs::File::open(target.parent().unwrap_or(Path::new(".")))?;
    let c_dst = c_string(target.file_name().unwrap_or_default())?;
    // SAFETY: the descriptors are open and both names are NUL-terminated.
    let cloned = unsafe {
        libc::clonefileat(
            parent,
            name.as_ptr(),
            dst_dir.as_raw_fd(),
            c_dst.as_ptr(),
            CLONE_NOFOLLOW,
        )
    };
    if cloned != 0 {
        return Err(io::Error::last_os_error());
    }
    if !std::fs::symlink_metadata(target)?.is_dir() {
        std::fs::remove_file(target)?;
        return Err(io::Error::from_raw_os_error(libc::ELOOP));
    }
    Ok(())
}

/// `clonefileat`'s flag that clones a final link as a link; `libc` does not name it.
#[cfg(target_os = "macos")]
const CLONE_NOFOLLOW: u32 = 0x0001;

#[cfg(target_os = "macos")]
fn sub_error(sub: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", sub.display()))
}

#[cfg(target_os = "macos")]
fn c_string(name: &std::ffi::OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path holds a NUL"))
}

/// Makes `to` with `clone` where the filesystem can, measuring what that took; `false`
/// when it cannot and a copy is needed.
#[cfg(target_os = "macos")]
fn try_clone(
    to: &Path,
    cap: u64,
    bytes: &mut u64,
    clone: impl FnOnce() -> io::Result<()>,
) -> Result<bool, CopyDirError> {
    if !to
        .parent()
        .is_none_or(|parent| std::fs::create_dir_all(parent).is_ok())
        || clone().is_err()
    {
        return Ok(false);
    }
    // The clone keeps the source's modes, and a read-only directory could not be
    // deleted with the rest of the throwaway, so it is made writable before anything
    // else can stop the copy.
    make_dirs_writable(to, 0).map_err(walk_stop)?;
    // A clone is one call, so what it took is measured after.
    *bytes = bytes.saturating_add(
        measure_host_subdir_skipping_vanished(to, Path::new("")).map_err(walk_stop)?,
    );
    if *bytes > cap {
        return Err(CopyDirError::OverCap);
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
fn clonefile(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (c_string(from.as_os_str())?, c_string(to.as_os_str())?);
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
    use crate::session_manager::CapabilitySettings;
    use crate::session_store::SqliteSessionStore;

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
        use crate::blueprint::BlueprintStore;
        let database = Arc::new(
            futures::executor::block_on(crate::database::ServerDatabase::open_ephemeral()).unwrap(),
        );
        futures::executor::block_on(
            crate::blueprint::SqliteBlueprintStore::new(database.clone(), None).add(blueprint()),
        )
        .unwrap();
        SessionManager::new(
            root.to_path_buf(),
            None,
            Arc::new(VolumeRegistry::new(volumes, root.join("managed"))),
            Arc::new(|| -> Arc<dyn interpreter::stdlib::http::HttpClient> {
                panic!("no HTTP in this test")
            }),
            Arc::new(SqliteSessionStore::new(database, None, root.to_path_buf())),
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
        let session_dir = manager
            .session_vfs_root(&session)
            .await
            .unwrap()
            .expect("per_session dir");
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
            .map(|name| (name.clone(), source.clone(), vec![PathBuf::new()]))
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
            vec![(
                "v".to_owned(),
                root.path().join("absent"),
                vec![PathBuf::new()],
            )],
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
            vec![("v".to_owned(), root.path().to_owned(), vec![PathBuf::new()])],
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_clone_of_a_sub_path_lands_under_it_and_refuses_a_link_above_it() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(from.join("users/alice")).unwrap();
        std::fs::write(from.join("users/alice/a.txt"), "alice").unwrap();
        std::fs::write(from.join("users/other.txt"), "other").unwrap();
        let to = root.path().join("to");
        let mut bytes = 0;
        // `true` means the anchored clone itself did it, not the copy fallback.
        assert!(clone_subdir(&from, Path::new("users/alice"), &to, 1 << 20, &mut bytes).unwrap());
        assert_eq!(bytes, 5);
        assert_eq!(
            std::fs::read_to_string(to.join("users/alice/a.txt")).unwrap(),
            "alice"
        );
        assert!(!to.join("users/other.txt").exists());

        let outside = root.path().join("outside");
        std::fs::create_dir_all(outside.join("alice")).unwrap();
        std::fs::write(outside.join("alice/secret"), "secret").unwrap();
        std::fs::create_dir_all(from.join("linked")).unwrap();
        std::os::unix::fs::symlink(&outside, from.join("linked/via")).unwrap();
        let refused = root.path().join("refused");
        let result = clone_subtree(
            &from,
            Path::new("linked/via/alice"),
            &refused,
            1 << 20,
            &mut 0,
        );
        assert!(matches!(result, Err(CopyDirError::Io(_))));
        assert!(!refused.join("linked/via/alice/secret").exists());
        let result = clone_subtree(&from, Path::new("linked/via"), &refused, 1 << 20, &mut 0);
        assert!(matches!(result, Err(CopyDirError::Io(_))));
        let missing = root.path().join("missing");
        clone_subtree(&from, Path::new("users/bob"), &missing, 1 << 20, &mut 0).unwrap();
        assert!(missing.is_dir() && !missing.join("users/bob").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_name_that_is_a_link_at_clone_time_is_removed_and_the_clone_fails() {
        use std::os::fd::AsRawFd;

        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(&from).unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, from.join("swapped")).unwrap();
        let parent = open_host_subdir(&from, Path::new("")).unwrap().unwrap();
        let to = root.path().join("to");
        std::fs::create_dir_all(&to).unwrap();
        let name = c_string("swapped".as_ref()).unwrap();
        let error = clone_entry(parent.as_raw_fd(), &name, &to.join("swapped")).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ELOOP));
        assert!(
            std::fs::symlink_metadata(to.join("swapped")).is_err(),
            "the cloned link is removed"
        );
        assert!(outside.is_dir(), "what it pointed at is untouched");
    }

    fn user_blueprint(sub_path: &str) -> Blueprint {
        submilli_blueprint::parse(&format!(
            "name: x\nvariables:\n  user:\n    required: false\nvfs:\n  mode: per_session\n  mounts:\n    /data: {{mode: named, volume: data, subPath: \"{sub_path}\"}}\n"
        ))
        .expect("blueprint")
    }

    fn user_bindings(user: &str) -> VarBindings {
        [("user".to_owned(), user.to_owned())].into_iter().collect()
    }

    /// A `data` volume with `users/alice/a.txt` (5 bytes) and `users/bob/b.txt` (3 bytes).
    fn users_volume(root: &Path) -> (PathBuf, SessionManager) {
        let (data, reference) = (root.join("data"), root.join("reference"));
        std::fs::create_dir_all(data.join("users/alice")).unwrap();
        std::fs::create_dir_all(data.join("users/bob")).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("users/alice/a.txt"), "alice").unwrap();
        std::fs::write(data.join("users/bob/b.txt"), "bob").unwrap();
        let manager = manager(volumes(&data, &reference), &root.join("sessions"));
        (data, manager)
    }

    #[tokio::test]
    async fn only_the_mounted_sub_path_of_a_volume_is_copied() {
        let root = tempfile::tempdir().unwrap();
        let (data, manager) = users_volume(root.path());
        let blueprint = user_blueprint("users/${vars.user}");
        let copy = Throwaway::copy(&manager, None, &blueprint, &user_bindings("alice"))
            .await
            .expect("copy");
        assert_eq!(copy.report.volumes_copied, ["data"]);
        assert_eq!(copy.report.bytes_copied, 5, "alice's file only");
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_eq!(
            std::fs::read_to_string(in_copy.join("users/alice/a.txt")).unwrap(),
            "alice"
        );
        assert!(!in_copy.join("users/bob").exists());
        std::fs::write(in_copy.join("users/alice/a.txt"), "changed").unwrap();
        assert_eq!(
            std::fs::read(data.join("users/alice/a.txt")).unwrap(),
            b"alice"
        );
        assert_eq!(std::fs::read(data.join("users/bob/b.txt")).unwrap(), b"bob");
    }

    #[tokio::test]
    async fn a_volume_over_the_cap_is_copied_when_what_is_mounted_fits() {
        let root = tempfile::tempdir().unwrap();
        let (data, manager) = users_volume(root.path());
        std::fs::File::create(data.join("users/bob/big.bin"))
            .unwrap()
            .set_len(3 << 20)
            .unwrap();
        let blueprint = user_blueprint("users/${vars.user}");
        let copy =
            Throwaway::copy_capped(&manager, None, &blueprint, &user_bindings("alice"), 2 << 20)
                .await
                .expect("alice's tree fits");
        assert_eq!(copy.report.bytes_copied, 5);
        let error =
            Throwaway::copy_capped(&manager, None, &blueprint, &user_bindings("bob"), 2 << 20)
                .await
                .err()
                .expect("bob's does not");
        assert!(matches!(error, ThrowawayError::TooLarge { .. }), "{error}");
    }

    #[tokio::test]
    async fn overlapping_mounts_copy_the_outer_one_once() {
        let root = tempfile::tempdir().unwrap();
        let (_data, manager) = users_volume(root.path());
        let blueprint = submilli_blueprint::parse(
            "name: x\nvfs:\n  mode: per_session\n  mounts:\n    /a: {mode: named, volume: data, subPath: users}\n    /b: {mode: named, volume: data, subPath: users/alice}\n",
        )
        .expect("blueprint");
        let copy = Throwaway::copy(&manager, None, &blueprint, &VarBindings::new())
            .await
            .expect("copy");
        assert_eq!(copy.report.bytes_copied, 8, "alice's and bob's, each once");
    }

    fn limited_volumes(data: &Path, reference: &Path, limit: u64) -> VolumeTable {
        let mut table = volumes(data, reference);
        table.get_mut("data").unwrap().size_limit = SizeLimit::Bytes(limit);
        table
    }

    #[tokio::test]
    async fn a_test_run_is_held_to_the_volumes_real_usage_not_the_partial_copys() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(data.join("users/alice")).unwrap();
        std::fs::create_dir_all(data.join("big")).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("users/alice/a.txt"), "alice").unwrap();
        std::fs::write(data.join("big/blob"), vec![0u8; 90]).unwrap();
        let manager = manager(
            limited_volumes(&data, &reference, 100),
            &root.path().join("sessions"),
        );
        let blueprint = user_blueprint("users/${vars.user}");
        let vars = user_bindings("alice");

        let normal = manager
            .volume_registry()
            .quota("data")
            .await
            .unwrap()
            .unwrap();
        let copy = Throwaway::copy(&manager, None, &blueprint, &vars)
            .await
            .unwrap();
        assert_eq!(copy.report.bytes_copied, 5);
        let test_run = copy.volumes.quota("data").await.unwrap().unwrap();
        assert_eq!((normal.used(), test_run.used()), (95, 95));
        assert_eq!(test_run.limit(), 100);
        // A write that does not fit in a normal run does not fit in the test run.
        assert!(normal.reserve(10).is_err());
        assert!(test_run.reserve(10).is_err());
        assert!(test_run.reserve(5).is_ok());
    }

    #[tokio::test]
    async fn a_source_volume_that_cannot_be_measured_leaves_the_test_run_full() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("copy");
        std::fs::create_dir_all(&dir).unwrap();
        let table = VolumeTable::from([(
            "data".to_owned(),
            VolumeSpec {
                size_limit: SizeLimit::Bytes(100),
                ..VolumeSpec::local_path(&dir)
            },
        )]);
        let registry = VolumeRegistry::new(table, PathBuf::new())
            .with_usage_offsets(BTreeMap::from([("data".to_owned(), None)]));
        let quota = registry.quota("data").await.unwrap().unwrap();
        assert!(quota.is_unmeasured());
        assert!(quota.reserve(1).is_err());
    }

    #[tokio::test]
    async fn a_read_only_reference_outside_the_writable_sub_paths_is_copied_and_readable() {
        let root = tempfile::tempdir().unwrap();
        let (data, manager) = users_volume(root.path());
        let blueprint = submilli_blueprint::parse(
            "name: x\nvfs:\n  mode: per_session\n  mounts:\n    /w: {mode: named, volume: data, subPath: users/alice}\n    /r: {mode: named, volume: data, subPath: users/bob, access: read_only}\n",
        )
        .expect("blueprint");
        let copy = Throwaway::copy(&manager, None, &blueprint, &VarBindings::new())
            .await
            .expect("copy");
        assert_eq!(copy.report.bytes_copied, 8);
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_eq!(
            std::fs::read_to_string(in_copy.join("users/bob/b.txt")).unwrap(),
            "bob"
        );
        let vfs = crate::session_manager::build_vfs(
            &blueprint,
            &VarBindings::new(),
            copy.session_root.as_deref(),
            None,
            &copy.volumes,
        );
        assert!(vfs.is_ok());

        // As in a normal run, a read-only sub-path that is not there fails the run.
        std::fs::remove_dir_all(data.join("users/bob")).unwrap();
        let missing = Throwaway::copy(&manager, None, &blueprint, &VarBindings::new())
            .await
            .expect("a missing sub-path copies nothing");
        let host = missing.volumes.resolve("data", None).unwrap().host;
        assert!(!host.join("users/bob").exists());
        let vfs = crate::session_manager::build_vfs(
            &blueprint,
            &VarBindings::new(),
            missing.session_root.as_deref(),
            None,
            &missing.volumes,
        );
        assert!(vfs.is_err(), "the read-only mount's directory is missing");
    }

    #[test]
    fn sub_paths_inside_another_are_dropped_by_components_not_prefix() {
        let paths = ["a/b", "a", "ab", "a/b/c", "x/y", "x/y"].map(PathBuf::from);
        assert_eq!(outermost(&paths), ["a", "ab", "x/y"].map(PathBuf::from));
        let all = ["a", "", "b"].map(PathBuf::from);
        assert_eq!(outermost(&all), [PathBuf::new()]);
    }

    #[tokio::test]
    async fn a_reference_to_the_whole_volume_copies_all_of_it() {
        let root = tempfile::tempdir().unwrap();
        let (_data, manager) = users_volume(root.path());
        let blueprint = submilli_blueprint::parse(
            "name: x\nvfs:\n  mode: per_session\n  mounts:\n    /a: {mode: named, volume: data, subPath: users/alice}\n    /b: {mode: named, volume: data}\n",
        )
        .expect("blueprint");
        let copy = Throwaway::copy(&manager, None, &blueprint, &VarBindings::new())
            .await
            .expect("copy");
        assert_eq!(copy.report.bytes_copied, 8);
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert!(in_copy.join("users/bob/b.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_sub_path_through_a_link_is_refused_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let (data, manager) = users_volume(root.path());
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(&outside, data.join("users/carol")).unwrap();
        for sub_path in ["users/carol", "users/carol/inner"] {
            let error = Throwaway::copy(
                &manager,
                None,
                &user_blueprint(sub_path),
                &VarBindings::new(),
            )
            .await
            .err()
            .expect("refused");
            assert!(matches!(error, ThrowawayError::Io(_)), "{error}");
        }
    }

    #[tokio::test]
    async fn a_missing_sub_path_copies_nothing_and_the_run_makes_it_as_a_normal_run_does() {
        let root = tempfile::tempdir().unwrap();
        let (data, manager) = users_volume(root.path());
        let blueprint = user_blueprint("users/${vars.user}");
        let copy = Throwaway::copy(&manager, None, &blueprint, &user_bindings("dave"))
            .await
            .expect("copy");
        assert_eq!(copy.report.bytes_copied, 0);
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_eq!(std::fs::read_dir(&in_copy).unwrap().count(), 0);
        // The run's own mount creates it in the copy, as it would in the volume.
        let vfs = crate::session_manager::build_vfs(
            &blueprint,
            &user_bindings("dave"),
            copy.session_root.as_deref(),
            None,
            &copy.volumes,
        );
        assert!(vfs.is_ok());
        assert!(in_copy.join("users/dave").is_dir());
        assert!(!data.join("users/dave").exists(), "the volume is untouched");
    }

    #[test]
    fn a_sub_path_that_is_not_a_plain_relative_path_is_refused() {
        for bad in ["/etc", "a/../b", "..", "a//b", "", "a/./b", "a\\b"] {
            assert!(relative_path(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(relative_path(None).unwrap(), PathBuf::new());
        assert_eq!(relative_path(Some("a/b")).unwrap(), PathBuf::from("a/b"));
    }
}
