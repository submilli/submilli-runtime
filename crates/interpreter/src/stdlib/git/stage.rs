//! A Git operation's changes, staged inside the repository's directory and
//! published by rename, all or nothing.
//!
//! An operation that changes a repository writes nothing into `.git` or the
//! worktree while it works. New objects go to the stage's object store, which
//! reads the repository's own through `alternates`. References are read from
//! and written to a copy. The index, configuration and checked-out files are
//! staged as files. Publication then moves each changed file into place,
//! keeping what it replaces, and undoes every move if one fails, so a failed
//! operation leaves the repository as it was. If undoing fails too, the stage
//! stays behind with what it replaced, and the repository refuses further Git
//! operations until the host recovers it.
use super::meter::Meter;
use super::storage::{prepare_parent, validate_path};
use crate::runtime::DiskQuota;
use crate::runtime::fs::FileIdentity;
use crate::runtime::host::quota_exceeded_error;
use cap_std::fs::Dir;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wasmtime::{Result, bail};

/// The stage's object store; its `info/alternates` names the repository's.
pub(super) const OBJECTS: &str = "objects";
/// A copy of the repository's references, which the operation reads and writes.
pub(super) const REFS: &str = "refs-copy";
/// Staged files that replace one in `.git`: `index`, `config`.
const METADATA: &str = "metadata";
/// Staged checked-out files.
const WORKTREE: &str = "worktree";
/// What publication replaced, kept until it's over.
const OLD: &str = "old";
/// The longest loose reference Git writes: a symbolic ref to a long name.
const MAX_LOOSE_REF: u64 = 4096;

pub(super) struct Stage {
    name: String,
    pub dir: Dir,
    /// The host path of the stage, for gix.
    pub host: PathBuf,
    repo: Arc<Dir>,
    meter: Arc<Meter>,
    published: bool,
}

/// Checked-out files a publication replaces.
#[derive(Default)]
pub(super) struct WorktreeChange {
    /// Paths whose file goes away, or is replaced by a staged one.
    pub remove: Vec<String>,
    /// Paths staged under the stage's worktree, to be placed.
    pub place: Vec<String>,
}

/// One rename publication made, so it can be undone.
enum Step {
    /// `target` was moved to `old/<backup>`.
    Removed { target: PathBuf, backup: String },
    /// The staged `from` became `target`.
    Placed { target: PathBuf, from: PathBuf },
    /// A directory publication created.
    CreatedDirectory(PathBuf),
    /// An emptied directory publication removed.
    RemovedDirectory(PathBuf),
}

impl Stage {
    /// Creates the stage in `repo`, whose host path is `host`.
    pub(super) fn create(repo: &Arc<Dir>, host: &Path, meter: Arc<Meter>) -> Result<Self> {
        let name = format!(".git-submilli-{}", uuid::Uuid::new_v4());
        repo.create_dir(&name)?;
        let dir = repo.open_dir(&name)?;
        let stage = Self {
            host: host.join(&name),
            name,
            dir,
            repo: Arc::clone(repo),
            meter,
            published: false,
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

    /// Copies the references, `HEAD` and `packed-refs` of the `.git` directory
    /// `git` into the stage's reference copy.
    pub(super) fn copy_references(&self, git: &Dir, cancelled: &AtomicBool) -> Result<()> {
        let copy = self.dir.open_dir(REFS)?;
        for file in ["HEAD", "packed-refs"] {
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

    pub(super) fn has_metadata(&self, name: &str) -> Result<bool> {
        Ok(self.dir.try_exists(Path::new(METADATA).join(name))?)
    }

    /// Stages a checked-out file at `path`: a symlink to `contents`, or a
    /// regular file of `mode` with `contents`.
    pub(super) fn write_worktree_file(&self, path: &str, mode: u32, contents: &[u8]) -> Result<()> {
        validate_path(path)?;
        self.meter
            .syscalls(3 + Path::new(path).components().count() as u64);
        self.meter.io(contents.len() as u64);
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

    /// Moves every staged change into place, counting what it adds against
    /// `quota` and freeing what it replaces; on failure, undoes every move.
    pub(super) fn publish(
        mut self,
        worktree: Option<&WorktreeChange>,
        quota: Option<&DiskQuota>,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let plan = self.plan(worktree, cancelled)?;
        let reservation = match quota {
            Some(quota) => Some(plan.reserve(quota)?),
            None => None,
        };
        let release = |reservation: &Option<Reservation>| {
            if let (Some(quota), Some(reservation)) = (quota, reservation) {
                quota.release(reservation.reserved);
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            release(&reservation);
            bail!("git: operation cancelled");
        }
        // Once the first rename happens, finish or undo, even if cancelled:
        // no program may see half a change.
        let mut steps = Vec::new();
        let result = self.apply(&plan, &mut steps);
        // A rename, removal or directory per step, and cleaning up the stage.
        self.meter
            .syscalls(2 * steps.len() as u64 + plan.added.len() as u64 + 4);
        match result {
            Ok(()) => {
                self.published = true;
                if let (Some(quota), Some(reservation)) = (quota, &reservation) {
                    reservation.settle(quota);
                }
                let _ = self.repo.remove_dir_all(&self.name);
                Ok(())
            }
            Err(error) => {
                release(&reservation);
                if let Err(undo) = self.undo(&steps) {
                    // Keep the stage, with what it replaced, for the host.
                    self.published = true;
                    return Err(error.context(format!(
                        "git: publication failed and could not be undone ({undo}); the \
                         repository needs host recovery from {}",
                        self.name
                    )));
                }
                Err(error)
            }
        }
    }

    /// What publication will move, in order: objects first, since nothing
    /// refers to them until the rest is in place.
    fn plan(&self, worktree: Option<&WorktreeChange>, cancelled: &AtomicBool) -> Result<Plan> {
        let mut plan = Plan::default();
        let git = Path::new(".git");
        // Loose objects and packs.
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
                    Kind::Object,
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
                plan.add(&self.repo, staged, PathBuf::from(path), len, Kind::Worktree)?;
            }
        }
        for name in ["index", "config"] {
            let staged = Path::new(METADATA).join(name);
            if self.dir.try_exists(&staged)? {
                let len = self.dir.metadata(&staged)?.len();
                plan.add(&self.repo, staged, git.join(name), len, Kind::Metadata)?;
            }
        }
        // References that differ from the repository's, `HEAD` last.
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
        Ok(plan)
    }

    fn apply(&self, plan: &Plan, steps: &mut Vec<Step>) -> Result<()> {
        let old = self.dir.open_dir(OLD)?;
        for (number, removed) in plan.removed.iter().enumerate() {
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
        // a directory's place.
        let mut emptied: Vec<&Path> = plan
            .removed
            .iter()
            .filter(|removed| removed.kind == Kind::Worktree)
            .flat_map(|removed| removed.target.ancestors().skip(1))
            .filter(|directory| !directory.as_os_str().is_empty())
            .collect();
        emptied.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
        emptied.dedup();
        for directory in emptied {
            if self.repo.remove_dir(directory).is_ok() {
                record(steps, Step::RemovedDirectory(directory.to_path_buf()))?;
            }
        }
        for added in &plan.added {
            let mut created = Vec::new();
            let prepared = match added.kind {
                Kind::Worktree => prepare_parent(&self.repo, &added.target, &mut created),
                Kind::Object | Kind::Metadata => {
                    create_parents(&self.repo, &added.target, &mut created)
                }
            };
            // What was created is undone even if creating the rest failed.
            steps.extend(created.into_iter().map(Step::CreatedDirectory));
            prepared?;
            check_injected_failure(steps)?;
            if let Some(backup) = &added.replaces {
                self.repo.rename(&added.target, &old, backup)?;
                record(
                    steps,
                    Step::Removed {
                        target: added.target.clone(),
                        backup: backup.clone(),
                    },
                )?;
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

    fn undo(&self, steps: &[Step]) -> Result<()> {
        let old = self.dir.open_dir(OLD)?;
        for step in steps.iter().rev() {
            match step {
                Step::Placed { target, from } => self.repo.rename(target, &self.dir, from)?,
                Step::Removed { target, backup } => old.rename(backup, &self.repo, target)?,
                Step::CreatedDirectory(directory) => self.repo.remove_dir(directory)?,
                Step::RemovedDirectory(directory) => self.repo.create_dir(directory)?,
            }
        }
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if !self.published {
            let _ = self.repo.remove_dir_all(&self.name);
        }
    }
}

/// Notes a step publication took, so it can be undone.
fn record(steps: &mut Vec<Step>, step: Step) -> Result<()> {
    steps.push(step);
    check_injected_failure(steps)
}

/// Fails publication where a test asked it to; see [`FAIL_AFTER`].
fn check_injected_failure(steps: &[Step]) -> Result<()> {
    #[cfg(test)]
    if FAIL_AFTER.with(|after| after.get().is_some_and(|after| steps.len() >= after)) {
        bail!("git: publication failed (injected)");
    }
    let _ = steps;
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Fails publication once it has taken this many steps.
    pub(super) static FAIL_AFTER: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Object,
    Metadata,
    Worktree,
}

struct Added {
    from: PathBuf,
    target: PathBuf,
    len: u64,
    kind: Kind,
    /// The backup name of the file this replaces, if any.
    replaces: Option<String>,
    replaced: Option<(FileIdentity, u64)>,
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
            Ok(meta) => Some((FileIdentity::of(&meta)?, regular_len(&meta))),
            // A missing parent, or one that a removal will turn into a directory.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        let replaces = replaced.map(|_| format!("r{}", self.added.len()));
        self.added.push(Added {
            from,
            target,
            len,
            kind,
            replaces,
            replaced,
        });
        Ok(())
    }

    fn remove(&mut self, repo: &Dir, target: &Path) -> Result<()> {
        let meta = repo.symlink_metadata(target)?;
        if meta.is_dir() {
            bail!(
                "git: {} is a directory, not a checked-out file",
                target.display()
            );
        }
        self.removed.push(Removed {
            target: target.to_path_buf(),
            kind: Kind::Worktree,
            file: (FileIdentity::of(&meta)?, regular_len(&meta)),
        });
        Ok(())
    }

    fn replaced(&self) -> impl Iterator<Item = (FileIdentity, u64)> + '_ {
        self.removed
            .iter()
            .map(|removed| removed.file)
            .chain(self.added.iter().filter_map(|added| added.replaced))
    }

    /// Reserves what publication adds less what it frees right away, refusing
    /// what doesn't fit. A replaced file a program holds open stays on disk
    /// until closed, so it frees nothing yet.
    fn reserve(&self, quota: &DiskQuota) -> Result<Reservation> {
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
        quota
            .reserve(reserved)
            .map_err(|exceeded| quota_exceeded_error(format!("git: {exceeded}")))?;
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

fn same_contents(left: &Dir, left_path: &Path, right: &Dir, right_path: &Path) -> Result<bool> {
    let read = |dir: &Dir, path: &Path| -> Result<Option<Vec<u8>>> {
        match dir.open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(64 * 1024 * 1024).read_to_end(&mut bytes)?;
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    };
    Ok(read(left, left_path)? == read(right, right_path)?)
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
    if depth > 64 {
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
    if depth > 64 {
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
