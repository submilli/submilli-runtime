//! A repository opened where it is, for one Git operation.
//!
//! gix reads the repository's `.git` in place, under the lock that keeps one
//! operation at a time on it, once [`metadata_scan`](super::metadata_scan) and
//! [`pack_index_check`](super::pack_index_check) have refused what could make
//! gix read or write outside it, or loop. The repository's own configuration
//! never reaches gix: it opens with a fixed one. An operation that changes the
//! repository writes through a [`Stage`] and publishes at the end; see
//! [`stage`](super::stage).
use super::location::Location;
use super::lock::RepositoryLock;
use super::meter::Meter;
use super::stage::{Stage, WorktreeChange};
use crate::runtime::DiskQuota;
use cap_std::fs::Dir;
use gix::bstr::ByteSlice;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasmtime::{Result, bail};

/// The most any one count of Git's working memory may reach, whatever the
/// run has free: a blob, a diff, the paths of a tree.
pub const MAX_WORKING_BYTES: u64 = 50 * 1024 * 1024;
/// The most a fetch may bring when the volume has no size limit to bound it.
pub const MAX_TRANSFER_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Paths in a file set, a sanity bound: what binds first is the memory each
/// path takes, counted against [`Snapshot::max_bytes`].
pub const MAX_PATHS: usize = 1_000_000;
/// Paths one call to `add` may name.
pub const MAX_REQUESTED_PATHS: usize = 10_000;
/// Directory levels Git follows in a tree, the worktree, `.git` or references.
pub const MAX_NESTING: usize = 64;

/// Git refusing work that needs more memory than it has free. Fetch failures
/// are redacted, since a remote's response may hold anything; this one names
/// only Git's own limit, so it reaches the program as it is.
#[derive(Debug)]
pub struct MemoryLimit(String);

impl std::fmt::Display for MemoryLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "git: {} limit exceeded; it needs more memory than Git has free, so raise \
             max_execution_memory",
            self.0
        )
    }
}

impl std::error::Error for MemoryLimit {}

/// Refuses work that needs more memory than Git has, saying what to raise.
pub fn memory_limit(what: &str) -> wasmtime::Error {
    wasmtime::Error::new(MemoryLimit(what.to_owned()))
}

/// [`memory_limit`] as an I/O error, for readers gix drives.
pub fn memory_limit_io(what: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        MemoryLimit(what.to_owned()),
    )
}

/// Files and their contents, as tests compare a directory before and after.
#[cfg(test)]
pub type Files = BTreeMap<String, (u32, Vec<u8>)>;
/// Each path's mode and object id: a tree, index or worktree listing that holds
/// no file contents.
pub type Entries = BTreeMap<String, (u32, gix::ObjectId)>;

/// The `.git` a new repository starts with.
const SKELETON_CONFIG: &[u8] = b"[core]\n\trepositoryformatversion = 0\n\tbare = false\n";

/// What a repository is opened with.
pub struct Opening {
    pub cancelled: Arc<AtomicBool>,
    /// What the operation may count while it works; see `WorkingBudget`.
    pub max_bytes: u64,
    /// The volume's size limit, which staged and published files count against.
    pub quota: Option<Arc<DiskQuota>>,
    /// The work the operation does, counted for fuel.
    pub meter: Arc<Meter>,
}

#[cfg(test)]
impl Opening {
    /// No size limit, and work counted nowhere.
    pub fn unmetered(cancelled: Arc<AtomicBool>, max_bytes: u64) -> Self {
        Self {
            cancelled,
            max_bytes,
            quota: None,
            meter: Default::default(),
        }
    }
}

pub struct Snapshot {
    pub max_bytes: u64,
    pub(super) algorithm_fuel: Arc<super::work::AlgorithmWork>,
    reference_cache: RefCell<ReferenceCache>,
    pub(super) history_cache: Option<Arc<super::log_cache::Cache>>,
    pub repo: gix::Repository,
    pub dir: Arc<Dir>,
    /// The repository's own configuration, which Git parses itself; replaced
    /// in full when a remote changes.
    pub config: Vec<u8>,
    config_changed: bool,
    pub cancelled: Arc<AtomicBool>,
    pub pending_worktree: std::cell::RefCell<Option<WorktreeChange>>,
    /// Where an operation that changes the repository writes; `None` for one
    /// that only reads.
    stage: Option<Stage>,
    quota: Option<Arc<DiskQuota>>,
    /// Whether this operation created `.git`, and the bytes it wrote there.
    created: Option<u64>,
    /// Whether a `.git` this operation created stays when this is dropped:
    /// published, or holding what a publication that needs host recovery kept.
    keep_git: bool,
    /// The work this operation does, counted for fuel.
    pub meter: Arc<Meter>,
    // Dropped last: the repository stays held until everything above is gone.
    _lock: RepositoryLock,
}

#[derive(Default)]
struct ReferenceCache {
    directories: HashMap<PathBuf, HashSet<OsString>>,
    bytes: usize,
}

impl Snapshot {
    /// Opens the repository at `location`, which `lock` holds; `writes` gives
    /// it a stage.
    pub fn open(
        location: &Location,
        lock: RepositoryLock,
        opening: Opening,
        writes: bool,
    ) -> Result<Self> {
        Self::open_locked(location, lock, opening, writes, None)
    }

    /// [`open`](Self::open), taking the lock: for tests.
    #[cfg(test)]
    pub fn open_unmetered(
        location: &Location,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
        writes: bool,
    ) -> Result<Self> {
        let lock = RepositoryLock::acquire(location.identity, &cancelled)?;
        Self::open(
            location,
            lock,
            Opening::unmetered(cancelled, max_bytes),
            writes,
        )
    }

    /// Creates a repository at `location`, which `lock` holds, on `branch`,
    /// and opens it to write. Its `.git` is removed again unless the
    /// operation publishes.
    pub fn init(
        location: &Location,
        lock: RepositoryLock,
        branch: &str,
        opening: Opening,
    ) -> Result<Self> {
        validate_new_ref_name(branch)?;
        let dir = &location.dir;
        if dir.try_exists(".git")? {
            bail!("git.init: repository already exists");
        }
        dir.create_dir(".git")?;
        let head = format!("ref: refs/heads/{branch}\n");
        let result = (|| {
            dir.create_dir_all(".git/objects/pack")?;
            dir.create_dir_all(".git/objects/info")?;
            dir.create_dir_all(".git/refs/heads")?;
            dir.write(".git/HEAD", &head)?;
            dir.write(".git/config", SKELETON_CONFIG)?;
            let written = (head.len() + SKELETON_CONFIG.len()) as u64;
            opening.meter.syscalls(6);
            opening.meter.io(written);
            Self::open_locked(location, lock, opening, true, Some(written))
        })();
        if result.is_err() {
            let _ = dir.remove_dir_all(".git");
        }
        result
    }

    /// [`init`](Self::init), taking the lock: for tests.
    #[cfg(test)]
    pub fn init_unmetered(
        location: &Location,
        branch: &str,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
    ) -> Result<Self> {
        let lock = RepositoryLock::acquire(location.identity, &cancelled)?;
        Self::init(
            location,
            lock,
            branch,
            Opening::unmetered(cancelled, max_bytes),
        )
    }

    fn open_locked(
        location: &Location,
        lock: RepositoryLock,
        opening: Opening,
        writes: bool,
        created: Option<u64>,
    ) -> Result<Self> {
        let Opening {
            cancelled,
            max_bytes,
            quota,
            meter,
        } = opening;
        let dir = Arc::clone(&location.dir);
        let git = dir.open_dir(".git")?;
        let config = check_metadata(&dir, &git, writes, max_bytes, &cancelled, &meter)?;
        let mut repo = open_in_place(&location.git_dir()?, max_bytes)?;
        let stage = if writes {
            Some(attach_stage(
                &mut repo,
                location,
                &git,
                max_bytes,
                &cancelled,
                &meter,
                quota.clone(),
            )?)
        } else {
            None
        };
        let snapshot = Self {
            max_bytes,
            algorithm_fuel: Arc::new(super::work::AlgorithmWork::new(u64::MAX)),
            reference_cache: RefCell::new(ReferenceCache::default()),
            history_cache: None,
            repo,
            dir,
            config,
            config_changed: false,
            cancelled,
            pending_worktree: Default::default(),
            stage,
            quota,
            created,
            keep_git: false,
            meter,
            _lock: lock,
        };
        snapshot.remotes()?;
        Ok(snapshot)
    }

    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        Ok(())
    }

    fn stage(&self) -> Result<&Stage> {
        self.stage.as_ref().ok_or_else(|| {
            wasmtime::Error::msg("git: this operation may not change the repository")
        })
    }

    /// What a fetch may bring, on disk and in gix's memory: each of the
    /// spooled responses and the pack written from them may take half of
    /// what the size limit leaves.
    pub fn transfer(&self) -> super::transport::Transfer {
        let room = self.quota.as_ref().map_or(MAX_TRANSFER_BYTES, |quota| {
            quota.limit().saturating_sub(quota.used()) / 2
        });
        super::transport::Transfer {
            max_transfer_bytes: room.min(MAX_TRANSFER_BYTES),
            pack: super::pack_limits::Limits {
                max_records: (self.max_bytes / 256).max(1) as usize,
                max_object_bytes: self.max_bytes,
                max_chain_bytes: self.max_bytes,
            },
        }
    }

    /// The index: the one this operation staged, or the repository's.
    pub fn index(&self) -> Result<gix::index::State> {
        let staged = match &self.stage {
            Some(stage) if stage.has_metadata("index")? => read_bounded(
                &stage.dir,
                &Stage::metadata_relative("index"),
                self.max_bytes,
            )?,
            _ => None,
        };
        let bytes = match staged {
            Some(bytes) => bytes,
            None => match read_bounded(&self.dir, Path::new(".git/index"), self.max_bytes)? {
                Some(bytes) => bytes,
                None => return Ok(gix::index::State::new(gix::hash::Kind::Sha1)),
            },
        };
        self.meter.syscalls(2);
        self.meter.io(bytes.len() as u64);
        self.meter.hash(bytes.len() as u64);
        self.meter.parse(bytes.len() as u64);
        // gix's decoders trust the index's extensions; keep only what's checked.
        let bytes = super::index_limits::sanitize(&bytes, self.max_bytes, &self.cancelled)?;
        let (state, _) = gix::index::State::from_bytes(
            &bytes,
            filetime::FileTime::now(),
            gix::hash::Kind::Sha1,
            gix::index::decode::Options {
                thread_limit: Some(1),
                alloc_limit_bytes: Some(self.max_bytes as usize),
                ..Default::default()
            },
        )?;
        Ok(state)
    }

    /// Stages `state` as the index.
    pub fn write_index(&self, state: gix::index::State) -> Result<()> {
        let stage = self.stage()?;
        self.meter.syscalls(3);
        self.meter.elements(state.entries().len() as u64);
        gix::index::File::from_state(state, stage.metadata_path("index"))
            .write(Default::default())?;
        Ok(())
    }

    /// Points `HEAD` at the local branch `branch`.
    pub fn write_head(&self, branch: &str) -> Result<()> {
        self.invalidate_reference_cache()?;
        self.stage()?;
        std::fs::write(
            self.repo.refs.git_dir().join("HEAD"),
            format!("ref: refs/heads/{branch}\n"),
        )?;
        Ok(())
    }

    /// Replaces the repository's configuration, at publication.
    pub fn set_config(&mut self, config: Vec<u8>) -> Result<()> {
        self.stage()?;
        self.config = config;
        self.config_changed = true;
        Ok(())
    }

    /// Stages a checked-out file; see [`Stage::write_worktree_file`].
    pub fn stage_worktree_file(&self, path: &str, mode: u32, contents: &[u8]) -> Result<()> {
        self.stage()?.write_worktree_file(path, mode, contents)
    }

    /// A directory in the stage for fetched responses, which are never published.
    pub fn spool(&self) -> Result<Dir> {
        let stage = self.stage()?;
        match stage.dir.create_dir("spool") {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        Ok(stage.dir.open_dir("spool")?)
    }

    /// Counts reading and parsing `packed-refs`, which listing references
    /// does besides reading each loose one. The stage's copy is the same.
    pub fn meter_packed_references(&self) -> Result<()> {
        match self.dir.symlink_metadata(".git/packed-refs") {
            Ok(meta) => {
                self.meter.syscalls(2);
                self.meter.io(meta.len());
                self.meter.parse(meta.len());
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// The stage's object store, and the repository's through it, opened to
    /// be shared with the fetch transport, which gix moves between threads.
    pub fn thread_safe_objects(&self) -> Result<gix::odb::Store> {
        Ok(gix::odb::Store::at_opts(
            self.stage()?.host.join(super::stage::OBJECTS),
            gix::hash::Kind::Sha1,
            &mut std::iter::empty(),
            gix::odb::store::init::Options {
                use_multi_pack_index: false,
                alloc_limit_bytes: Some(self.max_bytes as usize),
                ..Default::default()
            },
        )?)
    }

    /// The size limit, and what the stage holds against it, for a fetch's spool.
    pub fn staged_quota(&self) -> Result<super::stage::StagedQuota> {
        Ok(self.stage()?.staged_quota())
    }

    /// The pack directory of the stage's object store, where fetch writes.
    pub fn staged_packs(&self) -> Result<Dir> {
        Ok(self.stage()?.dir.open_dir("objects/pack")?)
    }

    pub fn record_algorithm_fuel(&self, units: u64) -> Result<()> {
        self.algorithm_fuel.charge(units)
    }

    pub fn validate_reference_spelling(&self, name: &str) -> Result<()> {
        if name.is_empty()
            || Path::new(name)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("git: invalid reference path");
        }
        // The references gix reads: the stage's copy when there is one.
        let references =
            Dir::open_ambient_dir(self.repo.refs.git_dir(), cap_std::ambient_authority())?;
        let mut directory = PathBuf::new();
        for component in Path::new(name).components() {
            self.check_cancelled()?;
            let Component::Normal(component) = component else {
                bail!("git: invalid reference path");
            };
            let path = directory.join(component);
            self.record_algorithm_fuel(crate::runtime::fuel::SYSCALL.cost(1))?;
            if !references.try_exists(&path)? {
                return Ok(());
            }
            if !self.reference_component_is_exact(&references, &directory, component)? {
                bail!("git: reference spelling aliases an existing reference path");
            }
            directory = path;
        }
        Ok(())
    }

    fn reference_component_is_exact(
        &self,
        references: &Dir,
        directory: &Path,
        name: &OsStr,
    ) -> Result<bool> {
        self.record_algorithm_fuel(crate::runtime::fuel::SCAN.cost(name.len() as u64))?;
        let mut cache = self.reference_cache.try_borrow_mut().map_err(|_| {
            crate::runtime::host::fatal_host_error("git: reference cache is already borrowed")
        })?;
        if let Some(names) = cache.directories.get(directory) {
            return Ok(names.contains(name));
        }
        let mut names = HashSet::new();
        for entry in super::stage::open_relative(references, directory)?.entries()? {
            self.check_cancelled()?;
            self.record_algorithm_fuel(crate::runtime::fuel::SYSCALL.cost(1))?;
            let name = entry?.file_name();
            cache.bytes = cache.bytes.saturating_add(name.len().saturating_add(256));
            if cache.bytes as u64 > self.max_bytes {
                bail!("git: reference spelling cache resource limit exceeded");
            }
            self.record_algorithm_fuel(crate::runtime::fuel::SCAN.cost(name.len() as u64))?;
            names
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            names.insert(name);
        }
        let exact = names.contains(name);
        cache.bytes = cache
            .bytes
            .saturating_add(directory.as_os_str().len().saturating_add(256));
        if cache.bytes as u64 > self.max_bytes {
            bail!("git: reference spelling cache resource limit exceeded");
        }
        cache
            .directories
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        cache.directories.insert(directory.to_owned(), names);
        Ok(exact)
    }

    pub fn invalidate_reference_cache(&self) -> Result<()> {
        let mut cache = self.reference_cache.try_borrow_mut().map_err(|_| {
            crate::runtime::host::fatal_host_error("git: reference cache is already borrowed")
        })?;
        *cache = ReferenceCache::default();
        Ok(())
    }

    pub fn validate_reference_updates(&self, names: &[String]) -> Result<()> {
        let stage = self.stage()?;
        let probe_name = format!("probe-{}", uuid::Uuid::new_v4());
        stage.dir.create_dir(&probe_name)?;
        let probe = stage.dir.open_dir(&probe_name)?;
        let result = (|| {
            let mut references = std::collections::HashSet::new();
            for name in names {
                self.validate_reference_spelling(name)?;
                if !references.insert(name) {
                    continue;
                }
                let path = Path::new(name);
                if let Some(parent) = path.parent() {
                    create_probe_directories(&probe, parent)?;
                }
                // Check the complete batch on the volume's filesystem before
                // gix creates any refs; two new destinations can alias each other.
                probe.open_with(
                    path,
                    cap_std::fs::OpenOptions::new().write(true).create_new(true),
                )?;
            }
            Ok(())
        })();
        let _ = stage.dir.remove_dir_all(&probe_name);
        result
    }

    /// Publishes everything staged, counting it against the size limit.
    pub fn publish(mut self) -> Result<()> {
        let stage = self
            .stage
            .take()
            .ok_or_else(|| wasmtime::Error::msg("git: nothing to publish"))?;
        if self.config_changed {
            stage.write_metadata("config", &self.config)?;
        }
        let worktree = self.pending_worktree.get_mut().take();
        // A repository this operation created also counts the `.git` it began with.
        let created = match (&self.quota, self.created) {
            (Some(quota), Some(written)) => {
                quota.reserve(written).map_err(|exceeded| {
                    crate::runtime::host::quota_exceeded_error(format!("git: {exceeded}"))
                })?;
                written
            }
            _ => 0,
        };
        if let Err(error) = stage.publish(worktree.as_ref(), &self.cancelled) {
            if error.is::<super::stage::NeedsHostRecovery>() {
                // What the stage kept belongs to this `.git`: keep it, counted.
                self.keep_git = true;
            } else if let Some(quota) = &self.quota {
                quota.release(created);
            }
            return Err(error);
        }
        self.keep_git = true;
        Ok(())
    }

    /// Every worktree path with the id its contents would have as a blob. Files
    /// are hashed as they're read, never held whole.
    pub fn worktree(&self) -> Result<Entries> {
        let mut entries = Entries::new();
        let mut state = WalkState {
            bytes: 0,
            paths: 0,
            cancelled: &self.cancelled,
            max_bytes: self.max_bytes,
        };
        walk(
            &self.dir,
            "",
            true,
            &mut state,
            0,
            &mut |dir, name, path, meta, _| {
                self.meter.syscalls(2);
                self.meter.io(meta.len());
                self.meter.hash(meta.len());
                self.meter.elements(1);
                let (mode, id) = hash_entry(dir, name, meta, &self.cancelled)?;
                entries.insert(path, (mode, id));
                // An id stands in for the contents.
                Ok(20)
            },
        )?;
        Ok(entries)
    }

    /// A worktree file's contents: a symlink's target, or at most `limit` bytes
    /// of an ordinary file.
    pub fn worktree_contents(&self, path: &str, limit: u64) -> Result<Vec<u8>> {
        validate_path(path)?;
        let meta = self.dir.symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return symlink_target(&self.dir, path);
        }
        let mut contents = Vec::new();
        self.dir
            .open(path)?
            .take(limit.saturating_add(1))
            .read_to_end(&mut contents)?;
        if contents.len() as u64 > limit {
            return Err(memory_limit(&format!("file size ({path})")));
        }
        self.meter.syscalls(2);
        self.meter.io(contents.len() as u64);
        Ok(contents)
    }

    /// Writes the worktree file at `path` into the object database as a blob,
    /// unless it's there already, streaming it rather than reading it whole.
    /// Returns its id, which differs from `expected` if the file changed since
    /// it was hashed.
    pub fn store_worktree_blob(
        &self,
        path: &str,
        mode: u32,
        expected: gix::ObjectId,
    ) -> Result<gix::ObjectId> {
        use gix::objs::Write as _;
        validate_path(path)?;
        if self.repo.has_object(expected) {
            return Ok(expected);
        }
        if mode == 0o120000 {
            return Ok(self
                .repo
                .write_blob(symlink_target(&self.dir, path)?)?
                .detach());
        }
        let file = self.dir.open(path)?;
        let len = file.metadata()?.len();
        // Read, hashed and deflated into a new loose object.
        self.meter.syscalls(4);
        self.meter.io(len.saturating_mul(2));
        self.meter.hash(len);
        self.meter.parse(len);
        let mut exact = ExactReader {
            inner: file.take(len),
            remaining: len,
        };
        let id = self
            .repo
            .objects
            .write_stream(gix::objs::Kind::Blob, len, &mut exact)
            .map_err(|error| wasmtime::Error::msg(format!("git: {path}: {error}")))?;
        if exact.remaining != 0 {
            bail!("git: {path} changed while it was being staged");
        }
        Ok(id)
    }

    pub fn remotes(&self) -> Result<BTreeMap<String, String>> {
        let config = gix::config::File::from_bytes_no_includes(
            &self.config,
            gix::config::file::Metadata::default(),
            Default::default(),
        )?;
        if config.string("extensions.objectFormat").is_some()
            || config.string("core.worktree").is_some()
            || config.string("extensions.worktreeConfig").is_some()
        {
            bail!("git: extended or externally located repositories are unsupported");
        }
        let mut remotes = BTreeMap::new();
        if let Some(sections) = config.sections_by_name("remote") {
            for section in sections {
                if let Some(name) = section.header().subsection_name() {
                    let name = name.to_str()?.to_owned();
                    validate_branch(&name)?;
                    if let Some(url) =
                        config.string_by("remote", Some(name.as_bytes().as_bstr()), "url")
                    {
                        let url = super::transport::canonical_url(url.to_str()?)?;
                        remotes.insert(name, url);
                    }
                }
            }
        }
        Ok(remotes)
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // The stage goes first, then a repository this operation created and
        // didn't publish; the lock is released last.
        drop(self.stage.take());
        if self.created.is_some() && !self.keep_git {
            let _ = self.dir.remove_dir_all(".git");
        }
    }
}

/// Everything that must hold before gix opens `.git`, the repository directory
/// `dir`'s, for an operation that `writes` or doesn't: no stage left by an
/// unfinished publication (a change clears any other stage left behind);
/// nothing in `.git`
/// that could lead gix outside it or into a loop, and a configuration small
/// enough to parse. Returns the configuration.
fn check_metadata(
    dir: &Dir,
    git: &Dir,
    writes: bool,
    max_bytes: u64,
    cancelled: &AtomicBool,
    meter: &Meter,
) -> Result<Vec<u8>> {
    let metadata = dir.symlink_metadata(".git")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("git: expected an ordinary .git directory");
    }
    super::stage::check_stages(dir, writes)?;
    let summary = super::metadata_scan::scan(git, cancelled, meter)?;
    validate_config_budget(&summary.config, max_bytes, cancelled)?;
    if !summary.packs.is_empty() {
        super::pack_index_check::check(
            &git.open_dir("objects/pack")?,
            &summary.packs,
            super::pack_index_check::Limits {
                max_objects: max_bytes / super::pack_index_check::check_bytes(1),
                max_object_bytes: max_bytes,
            },
            cancelled,
            meter,
        )?;
    }
    Ok(summary.config)
}

/// Gives `repo` a stage to write to: new objects go to the stage's object
/// store, references and `shallow` to the stage's copy of them.
fn attach_stage(
    repo: &mut gix::Repository,
    location: &Location,
    git: &Dir,
    max_bytes: u64,
    cancelled: &AtomicBool,
    meter: &Arc<Meter>,
    quota: Option<Arc<DiskQuota>>,
) -> Result<Stage> {
    let stage = Stage::create(&location.dir, &location.host, Arc::clone(meter), quota)?;
    stage.copy_references(git, cancelled)?;
    let objects = gix::odb::at_opts(
        stage.host.join(super::stage::OBJECTS),
        gix::hash::Kind::Sha1,
        [],
        gix::odb::store::init::Options {
            use_multi_pack_index: false,
            alloc_limit_bytes: Some(max_bytes as usize),
            ..Default::default()
        },
    )?;
    // Written objects go to the stage's store, not to memory.
    repo.objects = gix::odb::memory::Proxy::from(objects).with_write_passthrough();
    let references = stage.host.join(super::stage::REFS);
    // Fetch rewrites `shallow` where gix finds it; let that be the copy.
    let shallow = references.join("shallow");
    let shallow = shallow
        .to_str()
        .ok_or_else(|| wasmtime::Error::msg("git: non-UTF-8 repository path"))?;
    let mut config = repo.config_snapshot_mut();
    config.set_raw_value_by("gitoxide", Some("core".into()), "shallowFile", shallow)?;
    config.commit()?;
    repo.refs = reference_store(references);
    Ok(stage)
}

/// Creates the directories `parent` in `probe`, refusing one that a different
/// spelling already created on a case-folding or normalizing filesystem.
fn create_probe_directories(probe: &Dir, parent: &Path) -> Result<()> {
    let mut prefix = PathBuf::new();
    for component in parent.components() {
        let above = prefix.clone();
        prefix.push(component);
        match probe.create_dir(&prefix) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let above = super::stage::open_relative(probe, &above)?;
                if !super::stage::has_exact_entry(&above, component.as_os_str())? {
                    return Err(error.into());
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// The configuration gix runs with, in place of the repository's own, as
/// (section, subsection, key, value).
fn fixed_configuration(
    max_bytes: u64,
) -> [(&'static str, Option<&'static str>, &'static str, String); 9] {
    [
        (
            "gitoxide",
            Some("objects"),
            "allocLimit",
            max_bytes.to_string(),
        ),
        ("core", None, "logAllRefUpdates", "false".into()),
        ("index", None, "threads", "1".into()),
        ("core", None, "commitGraph", "false".into()),
        ("core", None, "multiPackIndex", "false".into()),
        ("core", None, "useReplaceRefs", "false".into()),
        ("core", None, "deltaBaseCacheLimit", "0".into()),
        ("gitoxide", Some("objects"), "cacheLimit", "0".into()),
        // Ignore server ACK IDs instead of letting them introduce
        // unvalidated local histories into the negotiation graph.
        ("fetch", None, "negotiationAlgorithm", "noop".into()),
    ]
}

/// Opens the `.git` at `git_dir` with a fixed configuration: the repository's
/// own is read for nothing but locating the repository. Paths, helpers,
/// includes, filters, hooks and external configuration are absent; remotes
/// are parsed separately, without includes.
fn open_in_place(git_dir: &Path, max_bytes: u64) -> Result<gix::Repository> {
    let fixed = fixed_configuration(max_bytes);
    let overrides: Vec<String> = fixed
        .iter()
        .map(|(section, subsection, key, value)| match subsection {
            Some(subsection) => format!("{section}.{subsection}.{key}={value}"),
            None => format!("{section}.{key}={value}"),
        })
        .collect();
    let mut repo = gix::open_opts(
        git_dir,
        gix::open::Options::isolated()
            .open_path_as_is(true)
            .with(gix::sec::Trust::Reduced)
            .filter_config_section(|meta| {
                !matches!(
                    meta.source,
                    gix::config::Source::Local | gix::config::Source::Worktree
                )
            })
            .config_overrides(overrides.iter().map(String::as_str)),
    )?;
    // Some values are read from the resolved configuration without the filter;
    // replace it with the fixed configuration alone.
    let mut config = gix::config::File::new(gix::config::file::Metadata::api());
    for (section, subsection, key, value) in [
        ("core", None, "repositoryformatversion", "0".to_owned()),
        ("core", None, "bare", "false".into()),
        ("core", None, "filemode", "true".into()),
    ]
    .into_iter()
    .chain(fixed)
    {
        config.set_raw_value_by(section, subsection.map(Into::into), key, value.as_str())?;
    }
    let mut snapshot = repo.config_snapshot_mut();
    *snapshot = config;
    snapshot.commit()?;
    repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
    Ok(repo)
}

/// A reference store over `git_dir` that writes no reflog.
fn reference_store(git_dir: PathBuf) -> gix::RefStore {
    gix::refs::file::Store::at_opts(
        git_dir,
        gix::hash::Kind::Sha1,
        gix::refs::store::init::Options {
            write_reflog: gix::refs::store::WriteReflog::Disable,
            ..Default::default()
        },
    )
}

/// The file at `path` in `dir`, refused past `limit` bytes; `None` if there
/// is none.
pub(super) fn read_bounded(dir: &Dir, path: &Path, limit: u64) -> Result<Option<Vec<u8>>> {
    let file = match dir.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(memory_limit(&format!("metadata size ({})", path.display())));
    }
    Ok(Some(bytes))
}

pub(super) fn validate_metadata_path(path: &str) -> Result<()> {
    validate_path(path)?;
    let mut components = path.split('/');
    let root = components
        .next()
        .ok_or_else(|| crate::runtime::host::invariant_trap("git: validated path has no root"))?;
    // A filesystem may fold case or ignore Unicode characters: refuse a
    // structural name spelled any way but its own, which gix might take for
    // the name, before gix reads `.git`.
    let uppercase_roots = [
        "HEAD",
        "ORIG_HEAD",
        "FETCH_HEAD",
        "MERGE_HEAD",
        "MERGE_MSG",
        "MERGE_MODE",
        "AUTO_MERGE",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "BISECT_LOG",
        "BISECT_NAMES",
        "BISECT_EXPECTED_REV",
        "BISECT_START",
        "BISECT_TERMS",
        "COMMIT_EDITMSG",
        "SQUASH_MSG",
        "TAG_EDITMSG",
    ];
    let stem = root.strip_suffix(".lock").unwrap_or(root);
    if let Some(canonical) = uppercase_roots
        .iter()
        .find(|name| stem.eq_ignore_ascii_case(name))
    {
        if stem != *canonical {
            bail!("git: noncanonical repository metadata path: {path}");
        }
    } else {
        validate_metadata_component(root, path)?;
    }
    match root {
        "objects" | "info" => {
            for component in components {
                validate_metadata_component(component, path)?;
            }
        }
        "refs" => validate_reference_namespace(components.next(), path)?,
        "logs" => match components.next() {
            Some("HEAD") | None => {}
            Some("refs") => validate_reference_namespace(components.next(), path)?,
            Some(_) => bail!("git: unsupported reflog metadata path: {path}"),
        },
        _ => {}
    }
    Ok(())
}

fn validate_reference_namespace(namespace: Option<&str>, path: &str) -> Result<()> {
    if let Some(namespace) = namespace {
        validate_metadata_component(namespace, path)?;
    }
    // Everything after the namespace is an actual ref name. Preserve its case
    // and Unicode spelling, including branch names that contain directories.
    Ok(())
}

fn validate_metadata_component(component: &str, path: &str) -> Result<()> {
    if !component.is_ascii() || component.bytes().any(|byte| byte.is_ascii_uppercase()) {
        bail!("git: noncanonical repository metadata path: {path}");
    }
    Ok(())
}

fn validate_config_budget(bytes: &[u8], max_bytes: u64, cancelled: &AtomicBool) -> Result<()> {
    // Even an implicit boolean such as `a\n` creates several parser events.
    // Bound their expansion before gix allocates the parsed configuration.
    let mut remaining = max_bytes.saturating_sub(bytes.len() as u64);
    for _ in bytes.split(|byte| *byte == b'\n') {
        if cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        remaining = remaining
            .checked_sub(256)
            .ok_or_else(|| memory_limit("configuration memory"))?;
    }
    Ok(())
}

/// The longest branch or remote name a program can create, in bytes: a
/// filesystem's 255-byte limit on one path component, less the `.lock` suffix git
/// writes beside a ref while updating it. It also bounds how much a new name adds
/// to `.git`. Names already in a repository aren't held to it.
const MAX_REF_NAME_BYTES: usize = 250;

/// [`validate_branch`] for a name about to be written into `.git`.
pub fn validate_new_ref_name(name: &str) -> Result<()> {
    if name.len() > MAX_REF_NAME_BYTES {
        bail!("git: a new branch or remote name is at most {MAX_REF_NAME_BYTES} bytes");
    }
    validate_branch(name)
}

pub fn validate_branch(name: &str) -> Result<()> {
    gix::validate::reference::name(format!("refs/heads/{name}").as_bytes().as_bstr())?;
    if name.is_empty() || name.starts_with('-') || name.contains(['\n', '\r', '"', '\\']) {
        bail!("git: invalid branch or remote name");
    }
    Ok(())
}

pub fn reject_in_progress(dir: &Dir) -> Result<()> {
    // Each marker is a file or a directory native Git leaves while it works.
    for marker in [
        "rebase-apply",
        "rebase-merge",
        "sequencer",
        "CHERRY_PICK_HEAD",
        "MERGE_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
    ] {
        match dir.symlink_metadata(format!(".git/{marker}")) {
            Ok(_) => bail!(
                "git: finish or abort the in-progress native Git operation before modifying this repository"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains(['\\', '\0'])
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path
            .split('/')
            .any(|c| c.eq_ignore_ascii_case(".git") || super::stage::is_reserved_stage_name(c))
    {
        bail!("git: invalid repository-relative path");
    }
    for component in path.split('/') {
        gix::validate::path::component(component.as_bytes().as_bstr(), None, Default::default())?;
    }
    Ok(())
}

#[cfg(test)]
pub fn read_files(
    dir: &Dir,
    worktree: bool,
    cancelled: &AtomicBool,
    max_bytes: u64,
) -> Result<Files> {
    let mut files = Files::new();
    let mut state = WalkState {
        bytes: 0,
        paths: 0,
        cancelled,
        max_bytes,
    };
    walk(
        dir,
        "",
        worktree,
        &mut state,
        0,
        &mut |dir, name, path, meta, remaining| {
            let (mode, content) = read_file_entry(dir, name, meta, remaining)?;
            let bytes = content.len() as u64;
            files.insert(path, (mode, content));
            Ok(bytes)
        },
    )?;
    Ok(files)
}

struct WalkState<'a> {
    bytes: u64,
    paths: usize,
    cancelled: &'a AtomicBool,
    max_bytes: u64,
}

/// Handles one file or symlink found by [`walk`]: given its directory, name,
/// repository-relative path, metadata and the bytes still available, returns
/// the bytes it kept in memory.
type VisitFile<'f> = dyn FnMut(&Dir, &str, String, &cap_std::fs::Metadata, u64) -> Result<u64> + 'f;

fn walk(
    dir: &Dir,
    prefix: &str,
    worktree: bool,
    state: &mut WalkState<'_>,
    depth: usize,
    visit: &mut VisitFile<'_>,
) -> Result<()> {
    if depth > MAX_NESTING {
        bail!("git: directory nesting limit exceeded");
    }
    for entry in dir.entries()? {
        if state.cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| wasmtime::Error::msg("git: non-UTF-8 paths are unsupported"))?;
        if worktree
            && (name.eq_ignore_ascii_case(".git") || super::stage::is_reserved_stage_name(&name))
        {
            if !prefix.is_empty() {
                bail!("git: nested repositories are unsupported");
            }
            if name != ".git" && name.eq_ignore_ascii_case(".git") {
                bail!("git: noncanonical worktree metadata path: {name}");
            }
            continue;
        }
        let path = format!("{prefix}{name}");
        state.bytes += path.len() as u64 + 128;
        if state.bytes > state.max_bytes {
            return Err(memory_limit("path memory"));
        }
        state.paths += 1;
        if state.paths > MAX_PATHS {
            return Err(memory_limit("repository path"));
        }
        let meta = dir.symlink_metadata(&name)?;
        if meta.is_dir() {
            walk(
                &dir.open_dir(&name)?,
                &format!("{path}/"),
                worktree,
                state,
                depth + 1,
                visit,
            )?;
            continue;
        }

        let remaining = state.max_bytes.saturating_sub(state.bytes);
        state.bytes += visit(dir, &name, path, &meta, remaining)?;
        if state.bytes > state.max_bytes {
            return Err(memory_limit("worktree memory"));
        }
    }
    Ok(())
}

/// The mode and blob id of a file or symlink, hashing the file as it's read.
fn hash_entry(
    dir: &Dir,
    name: &str,
    meta: &cap_std::fs::Metadata,
    cancelled: &AtomicBool,
) -> Result<(u32, gix::ObjectId)> {
    if meta.file_type().is_symlink() {
        let target = symlink_target(dir, name)?;
        let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, &target)?;
        return Ok((0o120000, id));
    }
    let mode = ordinary_file_mode(meta)?;
    let len = meta.len();
    let mut file = dir.open(name)?.take(len);
    let id = gix::objs::compute_stream_hash(
        gix::hash::Kind::Sha1,
        gix::objs::Kind::Blob,
        &mut file,
        len,
        &mut gix::progress::Discard,
        cancelled,
    )
    .map_err(|error| wasmtime::Error::msg(format!("git: hashing {name}: {error}")))?;
    Ok((mode, id))
}

fn symlink_target(dir: &Dir, name: &str) -> Result<Vec<u8>> {
    Ok(dir
        .read_link_contents(name)?
        .into_os_string()
        .into_string()
        .map_err(|_| wasmtime::Error::msg("git: non-UTF-8 symlink target"))?
        .into_bytes())
}

/// The mode of an ordinary file, refusing hard links and special files.
fn ordinary_file_mode(meta: &cap_std::fs::Metadata) -> Result<u32> {
    if !meta.is_file() {
        bail!("git: special files are unsupported");
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if meta.nlink() > 1 {
            bail!("git: hard-linked repository files are unsupported");
        }
    }
    #[cfg(unix)]
    let executable = {
        use cap_std::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(if executable { 0o100755 } else { 0o100644 })
}

/// Reads exactly `remaining` bytes, failing if the source ends sooner.
struct ExactReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> Read for ExactReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let read = self.inner.read(buf)?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        self.remaining -= read as u64;
        Ok(read)
    }
}

#[cfg(test)]
fn read_file_entry(
    dir: &Dir,
    name: &str,
    meta: &cap_std::fs::Metadata,
    remaining_bytes: u64,
) -> Result<(u32, Vec<u8>)> {
    if meta.file_type().is_symlink() {
        return Ok((0o120000, symlink_target(dir, name)?));
    }
    let mode = ordinary_file_mode(meta)?;
    let mut content = Vec::new();
    dir.open(name)?
        .take(remaining_bytes + 1)
        .read_to_end(&mut content)?;
    Ok((mode, content))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn metadata_paths_reject_filesystem_aliases_without_folding_ref_names() {
        for path in [
            "INDEX",
            "Config",
            "COMMONDIR",
            "Objects/info/alternates",
            "objects/Info/alternates",
            "objects/pack/PACK-a.idx",
            "refs/Heads/main",
            "head",
            "merge_head",
            "Rebase-apply/state",
            "bisect_log",
            "logs/head",
            "objects/info/alternates.",
            "index ",
            "objects\\info\\alternates",
            "con",
            "objects/pa\u{200c}ck/pack-a.idx",
            "inde\u{200c}x",
            "Konfig",
            "refs/hea\u{200c}ds/main",
        ] {
            assert!(validate_metadata_path(path).is_err(), "{path}");
        }
        for path in [
            "HEAD",
            "HEAD.lock",
            "MERGE_HEAD",
            "rebase-apply/state",
            "BISECT_LOG",
            "config",
            "index",
            "objects/pack/pack-a.pack",
            "objects/info/alternates",
            "refs/heads/Feature/Été",
            "refs/tags/Release",
            "refs/remotes/Origin/Main",
            "logs/HEAD",
            "logs/refs/heads/Feature/Été",
        ] {
            assert!(validate_metadata_path(path).is_ok(), "{path}");
        }
    }

    use super::super::location::Location;
    use super::super::stage::WorktreeChange;

    fn init(vfs: &crate::runtime::Vfs) -> Snapshot {
        Snapshot::init_unmetered(&Location::of_vfs(vfs), "main", Default::default(), 4096).unwrap()
    }

    fn open(vfs: &crate::runtime::Vfs, writes: bool) -> Result<Snapshot> {
        Snapshot::open_unmetered(&Location::of_vfs(vfs), Default::default(), 4096, writes)
    }

    /// Stages `files` as new checked-out files.
    fn stage_files(snapshot: &Snapshot, files: &[(&str, usize)]) {
        let mut change = WorktreeChange::default();
        for (path, len) in files {
            snapshot
                .stage_worktree_file(path, 0o100644, &vec![b'x'; *len])
                .unwrap();
            change.place.push((*path).to_owned());
        }
        *snapshot.pending_worktree.borrow_mut() = Some(change);
    }

    #[test]
    fn conflicting_pending_borrow_traps_before_publication() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish().unwrap();
        let snapshot = open(&vfs, true).unwrap();
        let original = snapshot.dir.read(".git/HEAD").unwrap();
        let borrowed = snapshot.pending_worktree.borrow_mut();
        let error =
            super::super::operations::replace_worktree(&snapshot, &Entries::new()).unwrap_err();
        assert!(error.is::<wasmtime::Trap>());
        assert_eq!(snapshot.dir.read(".git/HEAD").unwrap(), original);
        drop(borrowed);
        super::super::operations::replace_worktree(&snapshot, &Entries::new()).unwrap();
        snapshot.publish().unwrap();
    }

    #[test]
    fn snapshot_rejects_metadata_aliases_without_changing_source() {
        for path in [
            "INDEX",
            "Config",
            "COMMONDIR",
            "Objects/info/alternates",
            "objects/Pack/pack-a.idx",
            "merge_head",
            "Rebase-apply/state",
            "inde\u{200c}x",
        ] {
            let vfs = crate::runtime::Vfs::tempdir().unwrap();
            let root = vfs.dir().unwrap();
            root.create_dir(".git").unwrap();
            let metadata = root.open_dir(".git").unwrap();
            metadata.write("HEAD", "ref: refs/heads/main\n").unwrap();
            if let Some(parent) = Path::new(path).parent()
                && !parent.as_os_str().is_empty()
            {
                metadata.create_dir_all(parent).unwrap();
            }
            metadata.write(path, "untrusted metadata").unwrap();
            let cancelled = Arc::new(AtomicBool::new(false));
            let before = read_files(&metadata, false, &cancelled, 4096).unwrap();
            let error =
                Snapshot::open_unmetered(&Location::of_vfs(&vfs), cancelled.clone(), 4096, true)
                    .err()
                    .expect("metadata alias must be rejected before opening gix");
            assert!(
                error.to_string().contains("metadata path"),
                "{path}: {error}"
            );
            assert_eq!(
                read_files(&metadata, false, &cancelled, 4096).unwrap(),
                before
            );
        }
    }

    /// The repository in `vfs`, opened to write under `quota`, created if `init`.
    fn with_quota(
        vfs: &crate::runtime::Vfs,
        quota: &Arc<crate::runtime::DiskQuota>,
        init: bool,
    ) -> Result<Snapshot> {
        let location = Location::of_vfs(vfs);
        let cancelled = Arc::new(AtomicBool::new(false));
        let lock = RepositoryLock::acquire(location.identity, &cancelled)?;
        let opening = Opening {
            quota: Some(Arc::clone(quota)),
            ..Opening::unmetered(cancelled, 4096)
        };
        if init {
            Snapshot::init(&location, lock, "main", opening)
        } else {
            Snapshot::open(&location, lock, opening, true)
        }
    }

    #[test]
    fn a_checkout_is_refused_past_the_vfs_size_limit_as_it_is_staged() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let quota = Arc::new(crate::runtime::DiskQuota::new(500, 0));
        let snapshot = with_quota(&vfs, &quota, true).unwrap();
        let error = snapshot
            .stage_worktree_file("notes.txt", 0o100644, &[b'x'; 1000])
            .unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
        assert!(
            error
                .downcast_ref::<crate::runtime::host::QuotaExceededError>()
                .is_some(),
            "a refusal is a QuotaExceededError the program can catch"
        );
        drop(snapshot);
        assert_eq!(quota.used(), 0, "a refused checkout claims nothing");
        assert!(
            !vfs.dir().unwrap().try_exists(".git").unwrap(),
            "a repository that wasn't published is gone"
        );

        // With room, what publication adds is counted, and nothing else.
        let roomy = Arc::new(crate::runtime::DiskQuota::new(1 << 20, 0));
        let snapshot = with_quota(&vfs, &roomy, true).unwrap();
        stage_files(&snapshot, &[("notes.txt", 1000)]);
        snapshot.publish().unwrap();
        assert_eq!(
            roomy.used(),
            crate::runtime::measure_dir(vfs.dir().unwrap()).unwrap()
        );
    }

    #[test]
    fn a_publication_that_replaces_more_than_it_writes_frees_the_difference() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish().unwrap();
        let root = vfs.dir().unwrap();
        root.write("old.txt", vec![b'o'; 5000]).unwrap();
        let used = crate::runtime::measure_dir(root).unwrap();
        let tight = Arc::new(crate::runtime::DiskQuota::new(used + 1000, used));
        let snapshot = with_quota(&vfs, &tight, false).unwrap();
        stage_files(&snapshot, &[("new.txt", 1000)]);
        snapshot
            .pending_worktree
            .borrow_mut()
            .as_mut()
            .unwrap()
            .remove
            .push("old.txt".into());
        snapshot.publish().unwrap();
        assert_eq!(tight.used(), crate::runtime::measure_dir(root).unwrap());
    }

    #[test]
    fn a_new_repository_must_fit_the_size_limit() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let full = Arc::new(crate::runtime::DiskQuota::new(0, 0));
        let error = with_quota(&vfs, &full, true)
            .unwrap()
            .publish()
            .unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
    }

    #[test]
    fn snapshot_bounds_configuration_event_expansion() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish().unwrap();
        let root = vfs.dir().unwrap();
        root.write(".git/config", format!("[core]\n{}", "a\n".repeat(64)))
            .unwrap();
        assert!(
            open(&vfs, false)
                .err()
                .unwrap()
                .to_string()
                .contains("configuration memory limit")
        );
        root.write(
            ".git/config",
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        assert!(open(&vfs, false).is_ok());
    }

    #[test]
    fn a_read_leaves_the_repository_as_it_was() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish().unwrap();
        let root = vfs.dir().unwrap();
        let cancelled = AtomicBool::new(false);
        let before = read_files(root, false, &cancelled, 1 << 20).unwrap();
        let snapshot = open(&vfs, false).unwrap();
        super::super::operations::read(&snapshot, "status", &[]).unwrap();
        drop(snapshot);
        assert_eq!(
            read_files(root, false, &cancelled, 1 << 20).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn checkout_does_not_write_through_aliased_symlinks() {
        for (link, directory) in [("A", "a"), ("é", "e\u{301}")] {
            for mode in [0o100644, 0o120000] {
                let vfs = crate::runtime::Vfs::tempdir().unwrap();
                init(&vfs).publish().unwrap();
                let root = vfs.dir().unwrap();
                let head = root.read(".git/HEAD").unwrap();
                let snapshot = open(&vfs, true).unwrap();
                let target = format!("{directory}/HEAD");
                // Case-sensitive filesystems can keep both names. Aliasing
                // filesystems must fail without changing existing metadata.
                if snapshot
                    .stage_worktree_file(link, 0o120000, b".git")
                    .and_then(|()| snapshot.stage_worktree_file(&target, mode, b"overwrite"))
                    .is_ok()
                {
                    *snapshot.pending_worktree.borrow_mut() = Some(WorktreeChange {
                        remove: Vec::new(),
                        place: vec![link.to_owned(), target],
                    });
                    let _ = snapshot.publish();
                }
                assert_eq!(root.read(".git/HEAD").unwrap(), head);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn checkout_refuses_symlink_parents() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish().unwrap();
        let root = vfs.dir().unwrap();
        let head = root.read(".git/HEAD").unwrap();
        root.symlink_contents(".git", "alias").unwrap();
        let snapshot = open(&vfs, true).unwrap();
        stage_files(&snapshot, &[("alias/HEAD", 9)]);
        assert!(snapshot.publish().is_err());
        assert_eq!(root.read(".git/HEAD").unwrap(), head);
    }
}

#[cfg(test)]
mod alias_tests {
    use super::super::location::Location;
    use super::*;

    #[test]
    fn reference_updates_reject_existing_and_prospective_filesystem_aliases() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot =
            Snapshot::init_unmetered(&Location::of_vfs(&vfs), "main", Default::default(), 4096)
                .unwrap();
        for (first, second) in [("Main", "main"), ("é", "e\u{301}")] {
            let probe = tempfile::tempdir_in(vfs.root()).unwrap();
            std::fs::write(probe.path().join(first), "probe").unwrap();
            let aliases = probe.path().join(second).exists();
            for directory in [false, true] {
                let paths = if directory {
                    [
                        format!("refs/remotes/{first}/one"),
                        format!("refs/remotes/{second}/two"),
                    ]
                } else {
                    [
                        format!("refs/heads/{first}"),
                        format!("refs/heads/{second}"),
                    ]
                };
                assert_eq!(
                    snapshot.validate_reference_updates(&paths).is_err(),
                    aliases
                );
                let original = snapshot.repo.refs.git_dir().join(&paths[0]);
                std::fs::create_dir_all(original.parent().unwrap()).unwrap();
                std::fs::write(&original, "original").unwrap();
                assert_eq!(
                    snapshot.validate_reference_spelling(&paths[1]).is_err(),
                    aliases
                );
                assert_eq!(std::fs::read(&original).unwrap(), b"original");
                std::fs::remove_file(original).unwrap();
            }
        }
        assert!(
            snapshot
                .validate_reference_spelling("refs/heads/packed")
                .is_ok()
        );
    }

    #[test]
    fn worktree_rejects_root_metadata_aliases_without_changing_source() {
        for name in [".GIT", ".Git", ".gIt"] {
            let temp = tempfile::tempdir().unwrap();
            let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
            dir.create_dir(name).unwrap();
            dir.write(format!("{name}/keep"), "untracked data").unwrap();
            let error = read_files(&dir, true, &AtomicBool::new(false), 4096).unwrap_err();
            assert!(error.to_string().contains("noncanonical worktree metadata"));
            assert_eq!(dir.read(format!("{name}/keep")).unwrap(), b"untracked data");
        }
    }

    #[test]
    fn checkout_preserves_distinct_root_metadata_aliases() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let location = Location::of_vfs(&vfs);
        Snapshot::init_unmetered(&location, "main", Default::default(), 4096)
            .unwrap()
            .publish()
            .unwrap();
        let root = vfs.dir().unwrap();
        // A case-folding mount cannot hold this independent untracked directory.
        if root.try_exists(".GIT").unwrap() {
            return;
        }
        root.create_dir(".GIT").unwrap();
        root.write(".GIT/keep", "untracked data").unwrap();
        let before = root.read(".git/HEAD").unwrap();
        let snapshot = Snapshot::open_unmetered(&location, Default::default(), 4096, true).unwrap();
        let error =
            super::super::operations::replace_worktree(&snapshot, &Entries::new()).unwrap_err();
        assert!(error.to_string().contains("noncanonical worktree metadata"));
        assert_eq!(root.read(".GIT/keep").unwrap(), b"untracked data");
        assert_eq!(root.read(".git/HEAD").unwrap(), before);
        assert!(snapshot.pending_worktree.borrow().is_none());
    }
}
