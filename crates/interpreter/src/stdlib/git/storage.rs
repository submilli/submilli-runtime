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
use super::stage::{Stage, WorktreeChange};
use crate::runtime::DiskQuota;
use cap_std::fs::Dir;
use gix::bstr::ByteSlice;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasmtime::{Result, bail};

pub const MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_PATHS: usize = 10_000;
/// Files and their contents, as tests compare a directory before and after.
#[cfg(test)]
pub type Files = BTreeMap<String, (u32, Vec<u8>)>;
/// Each path's mode and object id: a tree, index or worktree listing that holds
/// no file contents.
pub type Entries = BTreeMap<String, (u32, gix::ObjectId)>;

/// The `.git` a new repository starts with.
const SKELETON_CONFIG: &[u8] = b"[core]\n\trepositoryformatversion = 0\n\tbare = false\n";

pub struct Snapshot {
    pub max_bytes: u64,
    pub repo: gix::Repository,
    pub dir: Arc<Dir>,
    /// The repository's own configuration, which Git parses itself; replaced
    /// in full when a remote changes.
    pub original_config: Vec<u8>,
    config_changed: bool,
    pub cancelled: Arc<AtomicBool>,
    pub pending_worktree: std::cell::RefCell<Option<WorktreeChange>>,
    /// Where an operation that changes the repository writes; `None` for one
    /// that only reads.
    stage: Option<Stage>,
    /// Whether this operation created `.git`, and the bytes it wrote there.
    created: Option<u64>,
    published: bool,
    // Dropped last: the repository stays held until everything above is gone.
    _lock: RepositoryLock,
}

impl Snapshot {
    /// Opens the repository at `location`; `writes` gives it a stage.
    pub fn open(
        location: &Location,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
        writes: bool,
    ) -> Result<Self> {
        let lock = RepositoryLock::acquire(location.identity, &cancelled)?;
        Self::open_locked(location, lock, cancelled, max_bytes, writes, None)
    }

    /// Creates a repository at `location`, on `branch`, and opens it to write.
    /// Its `.git` is removed again unless the operation publishes.
    pub fn init(
        location: &Location,
        branch: &str,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
    ) -> Result<Self> {
        validate_new_ref_name(branch)?;
        let lock = RepositoryLock::acquire(location.identity, &cancelled)?;
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
            Self::open_locked(location, lock, cancelled, max_bytes, true, Some(written))
        })();
        if result.is_err() {
            let _ = dir.remove_dir_all(".git");
        }
        result
    }

    fn open_locked(
        location: &Location,
        lock: RepositoryLock,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
        writes: bool,
        created: Option<u64>,
    ) -> Result<Self> {
        let dir = Arc::clone(&location.dir);
        let metadata = dir.symlink_metadata(".git")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("git: expected an ordinary .git directory");
        }
        for entry in dir.entries()? {
            if entry?
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(".git-submilli-")
            {
                bail!(
                    "git: unfinished publication requires host recovery before further Git operations"
                );
            }
        }
        let git = dir.open_dir(".git")?;
        let summary = super::metadata_scan::scan(&git, &cancelled)?;
        validate_config_budget(&summary.config, max_bytes, &cancelled)?;
        if !summary.packs.is_empty() {
            super::pack_index_check::check(
                &git.open_dir("objects/pack")?,
                &summary.packs,
                super::pack_index_check::Limits {
                    max_objects: max_bytes / super::pack_index_check::check_bytes(1),
                    max_object_bytes: max_bytes,
                },
                &cancelled,
            )?;
        }
        let mut repo = open_in_place(&location.git_dir()?, max_bytes)?;
        let stage = if writes {
            let stage = Stage::create(&dir, &location.host)?;
            stage.copy_references(&git, &cancelled)?;
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
            repo.refs = reference_store(stage.host.join(super::stage::REFS));
            Some(stage)
        } else {
            None
        };
        let snapshot = Self {
            max_bytes,
            repo,
            dir,
            original_config: summary.config,
            config_changed: false,
            cancelled,
            pending_worktree: Default::default(),
            stage,
            created,
            published: false,
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

    /// The index: the one this operation staged, or the repository's.
    pub fn index(&self) -> Result<gix::index::State> {
        let bytes = match &self.stage {
            Some(stage) if stage.has_metadata("index")? => {
                read_bounded(&stage.dir, Path::new("metadata/index"), self.max_bytes)?
            }
            _ => match self.dir.open(".git/index") {
                Ok(_) => read_bounded(&self.dir, Path::new(".git/index"), self.max_bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(gix::index::State::new(gix::hash::Kind::Sha1));
                }
                Err(error) => return Err(error.into()),
            },
        };
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
        gix::index::File::from_state(state, stage.metadata_path("index"))
            .write(Default::default())?;
        Ok(())
    }

    /// Points `HEAD` at the local branch `branch`.
    pub fn write_head(&self, branch: &str) -> Result<()> {
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
        self.original_config = config;
        self.config_changed = true;
        Ok(())
    }

    /// Stages a checked-out file; see [`Stage::write_worktree_file`].
    pub fn stage_worktree_file(&self, path: &str, mode: u32, contents: &[u8]) -> Result<()> {
        self.stage()?.write_worktree_file(path, mode, contents)
    }

    /// The pack directory of the stage's object store, where fetch writes.
    pub fn staged_packs(&self) -> Result<Dir> {
        Ok(self.stage()?.dir.open_dir("objects/pack")?)
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
        let mut directory = self.repo.refs.git_dir().to_path_buf();
        for component in Path::new(name).components() {
            self.check_cancelled()?;
            let Component::Normal(component) = component else {
                bail!("git: invalid reference path");
            };
            let path = directory.join(component);
            if !path.try_exists()? {
                return Ok(());
            }
            // Gix retains the requested spelling even when the filesystem
            // resolves a case or Unicode alias. Capability checks need the
            // actual ref spelling, including each directory component.
            let mut exact = false;
            for entry in std::fs::read_dir(&directory)? {
                self.check_cancelled()?;
                if entry?.file_name() == component {
                    exact = true;
                    break;
                }
            }
            if !exact {
                bail!("git: reference spelling aliases an existing reference path");
            }
            directory = path;
        }
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
                    let mut prefix = PathBuf::new();
                    for component in parent.components() {
                        prefix.push(component);
                        // A different spelling must not reuse a directory
                        // already created on a case-folding filesystem.
                        match probe.create_dir(&prefix) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                                let mut exact = false;
                                let above = prefix.parent().unwrap_or(Path::new(""));
                                let above = if above.as_os_str().is_empty() {
                                    probe.try_clone()?
                                } else {
                                    probe.open_dir(above)?
                                };
                                for entry in above.entries()? {
                                    if entry?.file_name() == component.as_os_str() {
                                        exact = true;
                                    }
                                }
                                if !exact {
                                    return Err(error.into());
                                }
                            }
                            Err(error) => return Err(error.into()),
                        }
                    }
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

    /// Publishes everything staged, counting it against `quota`.
    pub fn publish(mut self, quota: Option<&DiskQuota>) -> Result<()> {
        let stage = self
            .stage
            .take()
            .ok_or_else(|| wasmtime::Error::msg("git: nothing to publish"))?;
        if self.config_changed {
            stage.write_metadata("config", &self.original_config)?;
        }
        let worktree = self.pending_worktree.borrow_mut().take();
        if let (Some(quota), Some(written)) = (quota, self.created) {
            quota.reserve(written).map_err(|exceeded| {
                crate::runtime::host::quota_exceeded_error(format!("git: {exceeded}"))
            })?;
            if let Err(error) = stage.publish(worktree.as_ref(), Some(quota), &self.cancelled) {
                quota.release(written);
                return Err(error);
            }
        } else {
            stage.publish(worktree.as_ref(), quota, &self.cancelled)?;
        }
        self.published = true;
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
            bail!("git: file {path} exceeds the memory available to Git");
        }
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
            &self.original_config,
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
        if self.created.is_some() && !self.published {
            let _ = self.dir.remove_dir_all(".git");
        }
    }
}

/// Opens the `.git` at `git_dir` with a fixed configuration: the repository's
/// own is read for nothing but locating the repository. Paths, helpers,
/// includes, filters, hooks and external configuration are absent; remotes
/// are parsed separately, without includes.
fn open_in_place(git_dir: &Path, max_bytes: u64) -> Result<gix::Repository> {
    let overrides = [
        format!("gitoxide.objects.allocLimit={max_bytes}"),
        "core.logAllRefUpdates=false".into(),
        "index.threads=1".into(),
        "core.commitGraph=false".into(),
        "core.multiPackIndex=false".into(),
        "core.useReplaceRefs=false".into(),
        "core.deltaBaseCacheLimit=0".into(),
        "gitoxide.objects.cacheLimit=0".into(),
        // Ignore server ACK IDs instead of letting them introduce
        // unvalidated local histories into the negotiation graph.
        "fetch.negotiationAlgorithm=noop".into(),
    ];
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
    // replace it with the overrides alone.
    let mut text =
        String::from("[core]\nrepositoryformatversion = 0\nbare = false\nfilemode = true\n");
    for value in &overrides {
        let (key, value) = value.split_once('=').expect("override has a value");
        let (section, name) = key.rsplit_once('.').expect("override has a section");
        text.push_str(&format!("[{section}]\n{name} = {value}\n"));
    }
    let mut config = repo.config_snapshot_mut();
    *config = gix::config::File::from_bytes_owned(
        &mut text.into_bytes(),
        gix::config::file::Metadata::api(),
        Default::default(),
    )?;
    config.commit()?;
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

fn read_bounded(dir: &Dir, path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    dir.open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!(
            "git: {} is larger than the memory available to Git",
            path.display()
        );
    }
    Ok(bytes)
}

pub(super) fn validate_metadata_path(path: &str) -> Result<()> {
    validate_path(path)?;
    let mut components = path.split('/');
    let root = components.next().expect("validated nonempty path");
    // Scratch storage may fold case or ignore Unicode characters even when the
    // mounted repository does not. Classify structural names before copying.
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
            .ok_or_else(|| wasmtime::Error::msg("git: configuration memory limit exceeded"))?;
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
    // Check the original metadata: snapshots omit empty state directories.
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
        || path.split('/').any(|c| {
            c.eq_ignore_ascii_case(".git") || c.to_ascii_lowercase().starts_with(".git-submilli-")
        })
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
    if depth > 64 {
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
        if worktree && (name.eq_ignore_ascii_case(".git") || name.starts_with(".git-submilli-")) {
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
            bail!("git: path memory limit exceeded");
        }
        state.paths += 1;
        if state.paths > MAX_PATHS {
            bail!("git: repository path limit exceeded");
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
            bail!("git: repository snapshot exceeds tenant Git memory limit");
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

/// Creates the missing directories above the checked-out file `path`, each
/// under the exact spelling given, refusing to go through a link, a file, or a
/// directory spelled another way. Returns the directories created, outermost
/// first.
pub(super) fn prepare_parent(dir: &Dir, path: &Path) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    let Some(parent) = path.parent() else {
        return Ok(created);
    };
    let mut prefix = std::path::PathBuf::new();
    for component in parent.components() {
        prefix.push(component);
        match dir.symlink_metadata(&prefix) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let parent = prefix.parent().unwrap_or(Path::new(""));
                let parent = dir.open_dir(if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                })?;
                let mut exact = false;
                for entry in parent.entries()? {
                    if entry?.file_name() == component.as_os_str() {
                        exact = true;
                        break;
                    }
                }
                if !exact {
                    bail!("git: checkout directory spelling aliases an existing path");
                }
            }
            Ok(_) => bail!("git: checkout path overlaps a file or symlink"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                dir.create_dir(&prefix)?;
                created.push(prefix.clone());
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(created)
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
        Snapshot::init(&Location::of_vfs(vfs), "main", Default::default(), 4096).unwrap()
    }

    fn open(vfs: &crate::runtime::Vfs, writes: bool) -> Result<Snapshot> {
        Snapshot::open(&Location::of_vfs(vfs), Default::default(), 4096, writes)
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
            let error = Snapshot::open(&Location::of_vfs(&vfs), cancelled.clone(), 4096, true)
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

    #[test]
    fn publication_is_refused_past_the_vfs_size_limit() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = init(&vfs);
        stage_files(&snapshot, &[("notes.txt", 1000)]);
        let quota = crate::runtime::DiskQuota::new(500, 0);
        let error = snapshot.publish(Some(&quota)).unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
        assert!(
            error
                .downcast_ref::<crate::runtime::host::QuotaExceededError>()
                .is_some(),
            "a refusal is a QuotaExceededError the program can catch"
        );
        assert_eq!(quota.used(), 0, "a refused publication claims nothing");
        assert!(
            !vfs.dir().unwrap().try_exists(".git").unwrap(),
            "a repository that wasn't published is gone"
        );

        // With room, what publication adds is counted, and nothing else.
        let snapshot = init(&vfs);
        stage_files(&snapshot, &[("notes.txt", 1000)]);
        let roomy = crate::runtime::DiskQuota::new(1 << 20, 0);
        snapshot.publish(Some(&roomy)).unwrap();
        assert_eq!(
            roomy.used(),
            crate::runtime::measure_dir(vfs.dir().unwrap()).unwrap()
        );
    }

    #[test]
    fn a_publication_that_replaces_more_than_it_writes_needs_no_room() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish(None).unwrap();
        let root = vfs.dir().unwrap();
        root.write("old.txt", vec![b'o'; 5000]).unwrap();
        let used = crate::runtime::measure_dir(root).unwrap();
        let snapshot = open(&vfs, true).unwrap();
        stage_files(&snapshot, &[("new.txt", 1000)]);
        snapshot
            .pending_worktree
            .borrow_mut()
            .as_mut()
            .unwrap()
            .remove
            .push("old.txt".into());
        let tight = crate::runtime::DiskQuota::new(used + 10, used);
        snapshot.publish(Some(&tight)).unwrap();
        assert_eq!(tight.used(), crate::runtime::measure_dir(root).unwrap());
    }

    #[test]
    fn a_new_repository_must_fit_the_size_limit() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let full = crate::runtime::DiskQuota::new(0, 0);
        let error = init(&vfs).publish(Some(&full)).unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
    }

    #[test]
    fn snapshot_bounds_configuration_event_expansion() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish(None).unwrap();
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
        init(&vfs).publish(None).unwrap();
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
                init(&vfs).publish(None).unwrap();
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
                    let _ = snapshot.publish(None);
                }
                assert_eq!(root.read(".git/HEAD").unwrap(), head);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn checkout_refuses_symlink_parents() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        init(&vfs).publish(None).unwrap();
        let root = vfs.dir().unwrap();
        let head = root.read(".git/HEAD").unwrap();
        root.symlink_contents(".git", "alias").unwrap();
        let snapshot = open(&vfs, true).unwrap();
        stage_files(&snapshot, &[("alias/HEAD", 9)]);
        assert!(snapshot.publish(None).is_err());
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
            Snapshot::init(&Location::of_vfs(&vfs), "main", Default::default(), 4096).unwrap();
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
        Snapshot::init(&location, "main", Default::default(), 4096)
            .unwrap()
            .publish(None)
            .unwrap();
        let root = vfs.dir().unwrap();
        // A case-folding mount cannot hold this independent untracked directory.
        if root.try_exists(".GIT").unwrap() {
            return;
        }
        root.create_dir(".GIT").unwrap();
        root.write(".GIT/keep", "untracked data").unwrap();
        let before = root.read(".git/HEAD").unwrap();
        let snapshot = Snapshot::open(&location, Default::default(), 4096, true).unwrap();
        let error =
            super::super::operations::replace_worktree(&snapshot, &Entries::new()).unwrap_err();
        assert!(error.to_string().contains("noncanonical worktree metadata"));
        assert_eq!(root.read(".GIT/keep").unwrap(), b"untracked data");
        assert_eq!(root.read(".git/HEAD").unwrap(), before);
        assert!(snapshot.pending_worktree.borrow().is_none());
    }
}
