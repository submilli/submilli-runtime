//! A Git operation's changes, staged inside the repository's directory and
//! published by rename, all or nothing.
//!
//! An operation that changes a repository writes nothing into `.git` or the
//! worktree while it works. New objects go to the stage's object store, which
//! reads the repository's own through `alternates`. References and `shallow`
//! are read from and written to a copy. The index, configuration and
//! checked-out files are staged as files. Publication then moves each change
//! into place, keeping what it replaces, and undoes every move if one fails,
//! so a failed operation leaves the repository as it was.
//!
//! A stage marks itself before its first move and is removed, marker and all,
//! once the last is done. One repository is only ever worked on by one server
//! process, whose operations take turns on it (`lock.rs`), so a stage another
//! operation finds was left behind. Unmarked, by a crash before publication or
//! a cleanup that failed after it, it holds nothing the repository needs, and
//! the next change removes it; reads leave it, since a read may not write.
//! Marked, publication stopped part way: the repository refuses Git
//! operations until the host restores it.
use super::meter::Meter;
use super::storage::{MAX_NESTING, validate_path};
use crate::runtime::DiskQuota;
use crate::runtime::fs::FileIdentity;
use crate::runtime::host::quota_exceeded_error;
use cap_std::fs::Dir;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use wasmtime::{Result, bail};

/// What every stage's name starts with. No program may write a path holding
/// one (`protected_metadata` in `runtime/fs.rs`).
pub(super) const STAGE_PREFIX: &str = ".git-submilli-";
/// The stage's object store; its `info/alternates` names the repository's.
pub(super) const OBJECTS: &str = "objects";
/// A copy of the repository's references and `shallow`, which the operation
/// reads and writes.
pub(super) const REFS: &str = "refs-copy";
/// Staged files that replace one in `.git`: `index`, `config`.
const METADATA: &str = "metadata";
/// Staged checked-out files.
const WORKTREE: &str = "worktree";
/// What publication replaced, kept until it's over.
const OLD: &str = "old";
/// Present while publication is moving files: the stage holds what the
/// repository needs to be put back.
const PUBLISHING: &str = "publishing";
/// The longest loose reference Git writes: a symbolic ref to a long name.
const MAX_LOOSE_REF: u64 = 4096;
/// Files copied with the references, beside `refs/`.
const REFERENCE_FILES: [&str; 3] = ["HEAD", "packed-refs", "shallow"];

/// Whether `name` is one a stage could take, in any case: no checkout may
/// use it, nor may a program write it (`protected_metadata` in `runtime/fs.rs`).
pub(super) fn is_reserved_stage_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with(STAGE_PREFIX)
}

/// Whether `name` is exactly what [`Stage::create`] names a stage.
fn is_stage_directory_name(name: &str) -> bool {
    name.strip_prefix(STAGE_PREFIX)
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

/// Checks the stages in the repository directory `repo`, which this
/// operation holds: refuses the repository if a publication stopped part way,
/// and for an operation that `writes`, removes the stages left before one began.
pub(super) fn check_stages(repo: &Dir, writes: bool) -> Result<()> {
    for entry in repo.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| is_stage_directory_name(name)) else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if repo.open_dir(name)?.try_exists(PUBLISHING)? {
            bail!(
                "git: an unfinished publication in {name} requires host recovery before \
                 further Git operations"
            );
        }
        if writes {
            repo.remove_dir_all(name).map_err(|error| {
                wasmtime::Error::msg(format!(
                    "git: could not remove {name}, left by an earlier operation: {error}"
                ))
            })?;
        }
    }
    Ok(())
}

pub(super) struct Stage {
    name: String,
    pub dir: Dir,
    /// The host path of the stage, for gix.
    pub host: PathBuf,
    repo: Arc<Dir>,
    meter: Arc<Meter>,
    /// The volume's size limit, and what the stage holds against it.
    quota: StagedQuota,
    state: State,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Published,
    /// Publication failed and couldn't be undone: what it replaced stays for
    /// the host.
    NeedsRecovery,
}

/// Checked-out files a publication changes.
#[derive(Default)]
pub(super) struct WorktreeChange {
    /// Paths whose file goes away, including those a staged file replaces.
    pub remove: Vec<String>,
    /// Paths staged under the stage's worktree, to be placed once every
    /// removal is done.
    pub place: Vec<String>,
}

/// One move publication made, so it can be undone.
enum Step {
    /// `target` was moved to `old/<backup>`.
    Removed { target: PathBuf, backup: String },
    /// The staged `from` became `target`.
    Placed { target: PathBuf, from: PathBuf },
    /// A directory publication created.
    CreatedDirectory(PathBuf),
    /// An emptied directory publication removed, and its permissions.
    RemovedDirectory(PathBuf, cap_std::fs::Permissions),
}

impl Stage {
    /// Creates the stage in `repo`, whose host path is `host`.
    pub(super) fn create(
        repo: &Arc<Dir>,
        host: &Path,
        meter: Arc<Meter>,
        quota: Option<Arc<DiskQuota>>,
    ) -> Result<Self> {
        let name = format!("{STAGE_PREFIX}{}", uuid::Uuid::new_v4());
        repo.create_dir(&name)?;
        let dir = repo.open_dir(&name)?;
        let stage = Self {
            host: host.join(&name),
            name,
            dir,
            repo: Arc::clone(repo),
            meter,
            quota: StagedQuota {
                limit: quota,
                staged: Arc::default(),
            },
            state: State::Pending,
        };
        stage.meter.syscalls(8);
        for directory in [
            "objects/info",
            "objects/pack",
            REFS,
            METADATA,
            WORKTREE,
            OLD,
        ] {
            stage.dir.create_dir_all(directory)?;
        }
        // Relative to this object store, so no host path is written down.
        stage
            .dir
            .write("objects/info/alternates", "../../.git/objects\n")?;
        Ok(stage)
    }

    /// Copies the references, `HEAD`, `packed-refs` and `shallow` of the
    /// `.git` directory `git` into the stage's reference copy, streaming each.
    pub(super) fn copy_references(&self, git: &Dir, cancelled: &AtomicBool) -> Result<()> {
        let copy = self.dir.open_dir(REFS)?;
        for file in REFERENCE_FILES {
            match git.open(file) {
                Ok(source) => copy_file(source, &copy, Path::new(file), &self.meter)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        copy.create_dir("refs")?;
        match git.open_dir("refs") {
            Ok(refs) => copy_tree(
                &refs,
                &copy.open_dir("refs")?,
                Path::new(""),
                cancelled,
                0,
                &self.meter,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Writes `contents` as the staged replacement of `.git/<name>`.
    pub(super) fn write_metadata(&self, name: &str, contents: &[u8]) -> Result<()> {
        let mut file = self.dir.open_with(
            Path::new(METADATA).join(name),
            cap_std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true),
        )?;
        file.write_all(contents)?;
        self.meter.syscalls(2);
        self.meter.io(contents.len() as u64);
        Ok(())
    }

    /// The host path of the staged replacement of `.git/<name>`.
    pub(super) fn metadata_path(&self, name: &str) -> PathBuf {
        self.host.join(METADATA).join(name)
    }

    /// The staged replacement of `.git/<name>`, relative to the stage.
    pub(super) fn metadata_relative(name: &str) -> PathBuf {
        Path::new(METADATA).join(name)
    }

    pub(super) fn has_metadata(&self, name: &str) -> Result<bool> {
        Ok(self.dir.try_exists(Self::metadata_relative(name))?)
    }

    /// Stages a checked-out file at `path`: a symlink to `contents`, or a
    /// regular file of `mode` with `contents`. What it adds beyond the file it
    /// replaces counts against the size limit from now on, so a checkout too
    /// large for it stops here.
    pub(super) fn write_worktree_file(&self, path: &str, mode: u32, contents: &[u8]) -> Result<()> {
        validate_path(path)?;
        self.meter
            .syscalls(3 + Path::new(path).components().count() as u64);
        self.meter.io(contents.len() as u64);
        if mode != 0o120000 {
            let replaced = self.replaced_len(path)?;
            self.reserve((contents.len() as u64).saturating_sub(replaced))?;
        }
        let worktree = self.dir.open_dir(WORKTREE)?;
        if let Some(parent) = Path::new(path).parent() {
            worktree.create_dir_all(parent)?;
        }
        if mode == 0o120000 {
            let target = std::str::from_utf8(contents)?;
            #[cfg(unix)]
            worktree.symlink_contents(target, path)?;
            #[cfg(not(unix))]
            {
                let _ = target;
                bail!("git: symlink checkout is unsupported on this platform");
            }
            return Ok(());
        }
        let mut file = worktree.open_with(
            path,
            cap_std::fs::OpenOptions::new().write(true).create_new(true),
        )?;
        file.write_all(contents)?;
        #[cfg(unix)]
        {
            use cap_std::fs::PermissionsExt;
            file.set_permissions(cap_std::fs::Permissions::from_mode(if mode == 0o100755 {
                0o755
            } else {
                0o644
            }))?;
        }
        Ok(())
    }

    /// What publishing a file at `path` frees: the regular file there, unless
    /// a program holds it, which keeps it on disk, or it is reached through a
    /// link, which publication won't replace.
    fn replaced_len(&self, path: &str) -> Result<u64> {
        let Some(quota) = &self.quota.limit else {
            return Ok(0);
        };
        for ancestor in Path::new(path).ancestors().skip(1) {
            if ancestor.as_os_str().is_empty() {
                break;
            }
            match self.repo.symlink_metadata(ancestor) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                _ => return Ok(0),
            }
        }
        match self.repo.symlink_metadata(path) {
            Ok(meta) if meta.is_file() && !quota.is_held(FileIdentity::of(&meta)?) => {
                Ok(meta.len())
            }
            _ => Ok(0),
        }
    }

    /// Counts `bytes` the stage holds against the size limit until publication.
    fn reserve(&self, bytes: u64) -> Result<()> {
        self.quota
            .reserve(bytes)
            .map_err(|exceeded| quota_exceeded_error(format!("git: {exceeded}")))
    }

    /// Lets a fetch's spooled responses count against the size limit as they
    /// are written, with the rest of what the stage holds.
    pub(super) fn staged_quota(&self) -> StagedQuota {
        self.quota.clone()
    }

    /// Moves every staged change into place, counting what it adds against
    /// the size limit and freeing what it replaces; on failure, undoes every
    /// move.
    pub(super) fn publish(
        mut self,
        worktree: Option<&WorktreeChange>,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let plan = self.plan(worktree, cancelled)?;
        let quota = self.quota.limit.clone();
        // What publication needs replaces what staging held: the staged and
        // spooled files go with the stage.
        let reservation = match &quota {
            Some(quota) => {
                let staged = self.quota.take();
                match plan.reserve(quota, staged) {
                    Ok(reservation) => Some(reservation),
                    Err(error) => {
                        self.quota.restore(staged);
                        return Err(error);
                    }
                }
            }
            None => None,
        };
        let release = |reservation: &Option<Reservation>| {
            if let (Some(quota), Some(reservation)) = (&quota, reservation) {
                quota.release(reservation.reserved);
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            release(&reservation);
            bail!("git: operation cancelled");
        }
        // Once the first move happens, finish or undo, even if cancelled: no
        // program may see half a change.
        if let Err(error) = self.mark() {
            release(&reservation);
            return Err(error);
        }
        let mut steps = Vec::new();
        let result = self.apply(&plan, &mut steps);
        // A move, removal or directory per step, and the stage's cleanup.
        self.meter
            .syscalls(2 * steps.len() as u64 + plan.added.len() as u64 + 4);
        match result {
            Ok(()) => {
                if let (Some(quota), Some(reservation)) = (&quota, &reservation) {
                    reservation.settle(quota);
                }
                self.state = State::Published;
                // Unmarked first: a stage left behind unmarked holds nothing
                // the repository needs, and a later change removes it.
                let _ = self.dir.remove_file(PUBLISHING);
                let _ = self.repo.remove_dir_all(&self.name);
                Ok(())
            }
            Err(error) => {
                if let Err(undo) = self.undo(&steps) {
                    // Whatever stayed placed stays counted.
                    self.state = State::NeedsRecovery;
                    return Err(error.context(NeedsHostRecovery(format!(
                        "git: publication failed and could not be undone ({undo}); the \
                         repository needs host recovery from {}",
                        self.name
                    ))));
                }
                release(&reservation);
                let _ = self.dir.remove_file(PUBLISHING);
                Err(error)
            }
        }
    }

    /// Marks the stage as holding what the repository needs, durably, before
    /// the first move.
    fn mark(&self) -> Result<()> {
        let marker = self.dir.open_with(
            PUBLISHING,
            cap_std::fs::OpenOptions::new().write(true).create_new(true),
        )?;
        marker.sync_all()?;
        // So the marker's name is durable too, where a directory can be synced.
        if let Ok(stage) = self.dir.open(".") {
            let _ = stage.sync_all();
        }
        Ok(())
    }

    /// What publication will move, in order: new objects first, since nothing
    /// refers to them until the rest is in place, then the worktree, the
    /// index and configuration, and the references, `HEAD` last.
    fn plan(&self, worktree: Option<&WorktreeChange>, cancelled: &AtomicBool) -> Result<Plan> {
        let mut plan = Plan::default();
        let git = Path::new(".git");
        let objects = self.dir.open_dir(OBJECTS)?;
        for entry in objects.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| wasmtime::Error::msg("git: non-UTF-8 staged object"))?;
            if name == "info" || !entry.file_type()?.is_dir() {
                continue;
            }
            for object in objects.open_dir(name)?.entries()? {
                check_cancelled(cancelled)?;
                let object = object?;
                let file = object.file_name();
                let file = file
                    .to_str()
                    .ok_or_else(|| wasmtime::Error::msg("git: non-UTF-8 staged object"))?;
                if file.starts_with(".tmp") || file.ends_with(".keep") {
                    continue;
                }
                let relative = Path::new(name).join(file);
                let target = git.join("objects").join(&relative);
                // Objects are named by their contents: one already there is this one.
                if self.repo.try_exists(&target)? {
                    continue;
                }
                plan.add(
                    &self.repo,
                    Path::new(OBJECTS).join(&relative),
                    target,
                    object.metadata()?.len(),
                    Kind::Metadata,
                )?;
            }
        }
        if let Some(worktree) = worktree {
            for path in &worktree.remove {
                plan.remove(&self.repo, Path::new(path))?;
            }
            for path in &worktree.place {
                let staged = Path::new(WORKTREE).join(path);
                let len = regular_len(&self.dir.symlink_metadata(&staged)?);
                plan.added.push(Added {
                    from: staged,
                    target: PathBuf::from(path),
                    len,
                    kind: Kind::Worktree,
                    replaced: None,
                });
            }
        }
        for name in ["index", "config"] {
            let staged = Self::metadata_relative(name);
            if self.dir.try_exists(&staged)? {
                let len = self.dir.metadata(&staged)?.len();
                plan.add(&self.repo, staged, git.join(name), len, Kind::Metadata)?;
            }
        }
        // References and `shallow` that differ from the repository's, `HEAD`
        // last; a `shallow` the operation removed goes too.
        let copy = self.dir.open_dir(REFS)?;
        let mut references = Vec::new();
        collect_files(&copy, Path::new(""), &mut references, cancelled, 0)?;
        references.sort_by_key(|path| path == Path::new("HEAD"));
        for reference in references {
            if reference.extension().is_some_and(|ext| ext == "lock") {
                bail!(
                    "git: a reference update did not finish: {}",
                    reference.display()
                );
            }
            let target = git.join(&reference);
            self.meter.syscalls(4);
            if same_contents(&copy, &reference, &self.repo, &target)? {
                continue;
            }
            let len = copy.metadata(&reference)?.len();
            plan.add(
                &self.repo,
                Path::new(REFS).join(&reference),
                target,
                len,
                Kind::Metadata,
            )?;
        }
        if !copy.try_exists("shallow")? && self.repo.try_exists(".git/shallow")? {
            plan.remove_metadata(&self.repo, &git.join("shallow"))?;
        }
        Ok(plan)
    }

    fn apply(&self, plan: &Plan, steps: &mut Vec<Step>) -> Result<()> {
        let old = self.dir.open_dir(OLD)?;
        // Directories known to be real, exactly spelled, in the repository.
        let mut checked = HashSet::new();
        for (number, removed) in plan.removed.iter().enumerate() {
            if removed.kind == Kind::Worktree {
                check_parents(&self.repo, &removed.target, &mut checked, None)?;
            }
            let backup = number.to_string();
            self.repo.rename(&removed.target, &old, &backup)?;
            record(
                steps,
                Step::Removed {
                    target: removed.target.clone(),
                    backup,
                },
            )?;
        }
        // Directories the removals emptied, deepest first, so a file can take
        // a directory's place. One a placed file goes back into stays, as it
        // was.
        let refilled: HashSet<&Path> = plan
            .added
            .iter()
            .filter(|added| added.kind == Kind::Worktree)
            .flat_map(|added| added.target.ancestors().skip(1))
            .collect();
        let mut emptied: Vec<&Path> = plan
            .removed
            .iter()
            .filter(|removed| removed.kind == Kind::Worktree)
            .flat_map(|removed| removed.target.ancestors().skip(1))
            .filter(|directory| !directory.as_os_str().is_empty())
            .filter(|directory| !refilled.contains(directory))
            .collect();
        emptied.sort_by_key(|directory| {
            (
                std::cmp::Reverse(directory.components().count()),
                *directory,
            )
        });
        emptied.dedup();
        for directory in emptied {
            let permissions = self.repo.symlink_metadata(directory)?.permissions();
            if self.repo.remove_dir(directory).is_ok() {
                checked.remove(directory);
                record(
                    steps,
                    Step::RemovedDirectory(directory.to_path_buf(), permissions),
                )?;
            }
        }
        for added in &plan.added {
            let mut created = Vec::new();
            let prepared = match added.kind {
                Kind::Worktree => {
                    check_parents(&self.repo, &added.target, &mut checked, Some(&mut created))
                }
                Kind::Metadata => create_parents(&self.repo, &added.target, &mut created),
            };
            // What was created is undone even if creating the rest failed.
            steps.extend(created.into_iter().map(Step::CreatedDirectory));
            prepared?;
            check_injected_failure(steps)?;
            match added.replaced_backup() {
                Some(backup) => {
                    self.repo.rename(&added.target, &old, &backup)?;
                    record(
                        steps,
                        Step::Removed {
                            target: added.target.clone(),
                            backup,
                        },
                    )?;
                }
                // Every removal is done: anything still here is an alias of a
                // path just placed, or something no checkout should replace.
                None if exists(&self.repo, &added.target)? => bail!(
                    "git: {} is in the way of the checkout",
                    added.target.display()
                ),
                None => {}
            }
            self.dir.rename(&added.from, &self.repo, &added.target)?;
            record(
                steps,
                Step::Placed {
                    target: added.target.clone(),
                    from: added.from.clone(),
                },
            )?;
        }
        Ok(())
    }

    /// Undoes `steps`, latest first, going on past one that fails so as
    /// little as possible is left for the host; returns the first failure.
    fn undo(&self, steps: &[Step]) -> Result<()> {
        #[cfg(test)]
        if FAIL_UNDO.with(std::cell::Cell::get) {
            bail!("git: undo failed (injected)");
        }
        let old = self.dir.open_dir(OLD)?;
        let mut first_failure = None;
        for step in steps.iter().rev() {
            let undone = match step {
                Step::Placed { target, from } => self.repo.rename(target, &self.dir, from),
                Step::Removed { target, backup } => old.rename(backup, &self.repo, target),
                Step::CreatedDirectory(directory) => self.repo.remove_dir(directory),
                Step::RemovedDirectory(directory, permissions) => self
                    .repo
                    .create_dir(directory)
                    .and_then(|()| self.repo.set_permissions(directory, permissions.clone())),
            };
            if let Err(error) = undone {
                first_failure.get_or_insert(error);
            }
        }
        first_failure.map_or(Ok(()), |error| Err(error.into()))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if self.state == State::Pending {
            self.quota.release();
            let _ = self.repo.remove_dir_all(&self.name);
        }
    }
}

/// The size limit, and what a stage holds against it until publication:
/// staged checkouts, and a fetch's spooled responses, written away from the
/// stage. The default has no limit.
#[derive(Clone, Default)]
pub(super) struct StagedQuota {
    limit: Option<Arc<DiskQuota>>,
    staged: Arc<AtomicU64>,
}

impl StagedQuota {
    /// Counts `bytes` the stage holds, or refuses them past the limit.
    pub(super) fn reserve(&self, bytes: u64) -> Result<(), crate::runtime::QuotaExceeded> {
        if let Some(limit) = &self.limit {
            limit.reserve(bytes)?;
            self.staged.fetch_add(bytes, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Zeroes what is counted and returns it.
    fn take(&self) -> u64 {
        self.staged.swap(0, Ordering::Relaxed)
    }

    /// Gives back what [`take`](Self::take) took, publication having failed
    /// before reserving.
    fn restore(&self, bytes: u64) {
        self.staged.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Releases everything counted: the stage is gone unpublished.
    fn release(&self) {
        if let Some(limit) = &self.limit {
            limit.release(self.take());
        }
    }
}

/// A publication that failed and could not be undone: what it replaced is
/// in its stage, for the host to restore.
#[derive(Debug)]
pub(super) struct NeedsHostRecovery(String);

impl std::fmt::Display for NeedsHostRecovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NeedsHostRecovery {}

/// Notes a step publication took, so it can be undone.
fn record(steps: &mut Vec<Step>, step: Step) -> Result<()> {
    steps.push(step);
    check_injected_failure(steps)
}

/// Fails publication where a test asked it to; see [`FAIL_AFTER`].
#[cfg(test)]
fn check_injected_failure(steps: &[Step]) -> Result<()> {
    if FAIL_AFTER.with(|after| after.get().is_some_and(|after| steps.len() >= after)) {
        bail!("git: publication failed (injected)");
    }
    Ok(())
}

#[cfg(not(test))]
fn check_injected_failure(_steps: &[Step]) -> Result<()> {
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Fails publication once it has taken this many steps.
    pub(super) static FAIL_AFTER: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
    /// Fails undoing a failed publication.
    pub(super) static FAIL_UNDO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An object, reference or other file in `.git`, which no program can
    /// write to.
    Metadata,
    /// A checked-out file, in directories programs can change.
    Worktree,
}

struct Added {
    from: PathBuf,
    target: PathBuf,
    len: u64,
    kind: Kind,
    /// The file this replaces, by identity and size, with its number among
    /// those added.
    replaced: Option<(usize, FileIdentity, u64)>,
}

impl Added {
    fn replaced_backup(&self) -> Option<String> {
        self.replaced.map(|(number, _, _)| format!("r{number}"))
    }
}

struct Removed {
    target: PathBuf,
    kind: Kind,
    file: (FileIdentity, u64),
}

#[derive(Default)]
struct Plan {
    removed: Vec<Removed>,
    added: Vec<Added>,
}

impl Plan {
    /// Adds a file in `.git`, which replaces whatever is at `target` now.
    fn add(
        &mut self,
        repo: &Dir,
        from: PathBuf,
        target: PathBuf,
        len: u64,
        kind: Kind,
    ) -> Result<()> {
        let replaced = match repo.symlink_metadata(&target) {
            Ok(meta) if meta.is_dir() => bail!(
                "git: {} is a directory where a file is to be published",
                target.display()
            ),
            Ok(meta) => Some((
                self.added.len(),
                FileIdentity::of(&meta)?,
                regular_len(&meta),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        self.added.push(Added {
            from,
            target,
            len,
            kind,
            replaced,
        });
        Ok(())
    }

    /// Removes a checked-out file.
    fn remove(&mut self, repo: &Dir, target: &Path) -> Result<()> {
        self.remove_kind(repo, target, Kind::Worktree)
    }

    /// Removes a file in `.git`.
    fn remove_metadata(&mut self, repo: &Dir, target: &Path) -> Result<()> {
        self.remove_kind(repo, target, Kind::Metadata)
    }

    fn remove_kind(&mut self, repo: &Dir, target: &Path, kind: Kind) -> Result<()> {
        let meta = repo.symlink_metadata(target)?;
        if meta.is_dir() {
            bail!("git: {} is a directory, not a file", target.display());
        }
        self.removed.push(Removed {
            target: target.to_path_buf(),
            kind,
            file: (FileIdentity::of(&meta)?, regular_len(&meta)),
        });
        Ok(())
    }

    fn replaced(&self) -> impl Iterator<Item = (FileIdentity, u64)> + '_ {
        self.removed.iter().map(|removed| removed.file).chain(
            self.added
                .iter()
                .filter_map(|added| added.replaced.map(|(_, file, len)| (file, len))),
        )
    }

    /// Reserves what publication adds less what it frees right away, given
    /// that `staged` is reserved already, refusing what doesn't fit. A
    /// replaced file a program holds open stays on disk until closed, so it
    /// frees nothing yet.
    fn reserve(&self, quota: &DiskQuota, staged: u64) -> Result<Reservation> {
        if quota.is_unmeasured() {
            let refused = crate::runtime::QuotaExceeded::Unmeasured {
                limit: quota.limit(),
            };
            return Err(quota_exceeded_error(format!("git: {refused}")));
        }
        let added = self
            .added
            .iter()
            .fold(0u64, |sum, added| sum.saturating_add(added.len));
        let mut freed = 0u64;
        let mut held = Vec::new();
        for (file, len) in self.replaced() {
            if quota.is_held(file) {
                held.push((file, len));
            } else {
                freed = freed.saturating_add(len);
            }
        }
        let reserved = added.saturating_sub(freed);
        if reserved > staged {
            quota
                .reserve(reserved - staged)
                .map_err(|exceeded| quota_exceeded_error(format!("git: {exceeded}")))?;
        } else {
            quota.release(staged - reserved);
        }
        Ok(Reservation {
            reserved,
            excess: freed.saturating_sub(added),
            held,
        })
    }
}

/// What publication reserved against the size limit.
struct Reservation {
    reserved: u64,
    /// What publication frees beyond what it adds.
    excess: u64,
    /// Replaced files a program holds open, freed when it closes them.
    held: Vec<(FileIdentity, u64)>,
}

impl Reservation {
    fn settle(&self, quota: &DiskQuota) {
        quota.release(self.excess);
        for (file, len) in &self.held {
            quota.release_file(*file, *len);
        }
    }
}

fn regular_len(meta: &cap_std::fs::Metadata) -> u64 {
    if meta.is_file() { meta.len() } else { 0 }
}

/// Whether anything, a dangling link included, is at `path`.
fn exists(dir: &Dir, path: &Path) -> Result<bool> {
    match dir.symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

/// Whether the directory `dir` holds an entry spelled exactly `name`: a
/// case-folding or normalizing filesystem resolves other spellings too.
pub(super) fn has_exact_entry(dir: &Dir, name: &std::ffi::OsStr) -> Result<bool> {
    for entry in dir.entries()? {
        if entry?.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The directory `relative` in `dir`, `dir` itself when empty.
pub(super) fn open_relative(dir: &Dir, relative: &Path) -> Result<Dir> {
    Ok(if relative.as_os_str().is_empty() {
        dir.try_clone()?
    } else {
        dir.open_dir(relative)?
    })
}

/// Checks that the parent directories of the checked-out file `path` are
/// real directories under their exact spelling: a link, or an alias, could
/// lead elsewhere. With `created`, creates the missing ones, adding each to it
/// as it does, outermost first, so a failure part way can be undone; without,
/// a missing one is an error.
fn check_parents(
    dir: &Dir,
    path: &Path,
    checked: &mut HashSet<PathBuf>,
    mut created: Option<&mut Vec<PathBuf>>,
) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut prefix = PathBuf::new();
    for component in parent.components() {
        let above = prefix.clone();
        prefix.push(component);
        if checked.contains(&prefix) {
            continue;
        }
        match dir.symlink_metadata(&prefix) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                if !has_exact_entry(&open_relative(dir, &above)?, component.as_os_str())? {
                    bail!("git: checkout directory spelling aliases an existing path");
                }
            }
            Ok(_) => bail!("git: checkout path overlaps a file or symlink"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(created) = created.as_deref_mut() else {
                    return Err(error.into());
                };
                dir.create_dir(&prefix)?;
                created.push(prefix.clone());
            }
            Err(error) => return Err(error.into()),
        }
        checked.insert(prefix.clone());
    }
    Ok(())
}

/// Creates the missing directories above `path` in `repo`, adding each to
/// `created`, outermost first, as it creates it. Used inside `.git`, which no
/// program can write to.
fn create_parents(repo: &Dir, path: &Path, created: &mut Vec<PathBuf>) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut prefix = PathBuf::new();
    for component in parent.components() {
        prefix.push(component);
        match repo.symlink_metadata(&prefix) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => bail!("git: {} is not a directory", prefix.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                repo.create_dir(&prefix)?;
                created.push(prefix.clone());
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Whether `left_path` in `left` and `right_path` in `right` are both there
/// and hold the same bytes, compared as they're read.
fn same_contents(left: &Dir, left_path: &Path, right: &Dir, right_path: &Path) -> Result<bool> {
    let open = |dir: &Dir, path: &Path| -> Result<Option<cap_std::fs::File>> {
        match dir.open(path) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    };
    let (Some(left), Some(right)) = (open(left, left_path)?, open(right, right_path)?) else {
        return Ok(false);
    };
    if left.metadata()?.len() != right.metadata()?.len() {
        return Ok(false);
    }
    let (mut left, mut right) = (
        std::io::BufReader::new(left),
        std::io::BufReader::new(right),
    );
    let (mut left_chunk, mut right_chunk) = ([0u8; 8192], [0u8; 8192]);
    loop {
        let read = left.read(&mut left_chunk)?;
        if read == 0 {
            return Ok(right.read(&mut right_chunk[..1])? == 0);
        }
        right.read_exact(&mut right_chunk[..read])?;
        if left_chunk[..read] != right_chunk[..read] {
            return Ok(false);
        }
    }
}

fn copy_file(mut source: cap_std::fs::File, to: &Dir, path: &Path, meter: &Meter) -> Result<()> {
    let mut destination = to.open_with(
        path,
        cap_std::fs::OpenOptions::new().write(true).create_new(true),
    )?;
    let copied = std::io::copy(&mut source, &mut destination)?;
    meter.syscalls(3);
    meter.io(copied.saturating_mul(2));
    Ok(())
}

fn copy_tree(
    from: &Dir,
    to: &Dir,
    prefix: &Path,
    cancelled: &AtomicBool,
    depth: usize,
    meter: &Meter,
) -> Result<()> {
    if depth > MAX_NESTING {
        bail!("git: reference nesting limit exceeded");
    }
    for entry in from.entries()? {
        meter.syscalls(1);
        check_cancelled(cancelled)?;
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            to.create_dir(&name)?;
            copy_tree(
                &from.open_dir(&name)?,
                &to.open_dir(&name)?,
                &prefix.join(&name),
                cancelled,
                depth + 1,
                meter,
            )?;
        } else if kind.is_file() {
            let file = from.open(&name)?;
            if file.metadata()?.len() > MAX_LOOSE_REF {
                bail!(
                    "git: reference {} is larger than a reference can be",
                    prefix.join(&name).display()
                );
            }
            copy_file(file, to, Path::new(&name), meter)?;
        } else {
            bail!("git: special files are unsupported in references");
        }
    }
    Ok(())
}

fn collect_files(
    dir: &Dir,
    prefix: &Path,
    files: &mut Vec<PathBuf>,
    cancelled: &AtomicBool,
    depth: usize,
) -> Result<()> {
    if depth > MAX_NESTING {
        bail!("git: reference nesting limit exceeded");
    }
    for entry in dir.entries()? {
        check_cancelled(cancelled)?;
        let entry = entry?;
        let name = entry.file_name();
        let path = prefix.join(&name);
        if entry.file_type()?.is_dir() {
            collect_files(&dir.open_dir(&name)?, &path, files, cancelled, depth + 1)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        bail!("git: operation cancelled");
    }
    Ok(())
}
