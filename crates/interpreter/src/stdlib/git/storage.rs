//! Gix only sees a private metadata copy; VFS paths never become ambient paths.
use crate::runtime::host::range_error;
use crate::runtime::{DiskQuota, measure_with_held};
use cap_std::fs::Dir;
use gix::bstr::ByteSlice;
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasmtime::{Result, bail};

pub const MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_PATHS: usize = 10_000;
pub type Files = BTreeMap<String, (u32, Vec<u8>)>;

pub struct Snapshot {
    pub max_bytes: u64,
    created: bool,
    published: std::cell::Cell<bool>,
    pub temp: tempfile::TempDir,
    pub repo: gix::Repository,
    pub dir: Arc<Dir>,
    pub original_config: Vec<u8>,
    pub cancelled: Arc<AtomicBool>,
    pub pending_worktree: std::cell::RefCell<Option<Files>>,
}

impl Snapshot {
    pub fn open(dir: Arc<Dir>, cancelled: Arc<AtomicBool>, max_bytes: u64) -> Result<Self> {
        let temp = tempfile::tempdir()?;
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
        let files = read_files(&dir.open_dir(".git")?, false, &cancelled, max_bytes)?;
        for path in files.keys() {
            validate_metadata_path(path)?;
        }
        if files.contains_key("objects/info/alternates") || files.contains_key("commondir") {
            bail!("git: external object stores and linked worktrees are unsupported");
        }
        copy_metadata_to_scratch(temp.path(), &files, max_bytes, &cancelled)?;
        let original_config = files
            .get("config")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        validate_config_budget(&original_config, max_bytes, &cancelled)?;
        let repo = open_isolated_repository(temp.path(), max_bytes)?;
        Ok(Self {
            max_bytes,
            created: false,
            published: std::cell::Cell::new(false),
            temp,
            repo,
            dir,
            original_config,
            cancelled,
            pending_worktree: Default::default(),
        })
    }

    pub fn init(
        dir: Arc<Dir>,
        branch: &str,
        cancelled: Arc<AtomicBool>,
        max_bytes: u64,
    ) -> Result<Self> {
        validate_new_ref_name(branch)?;
        if dir.try_exists(".git")? {
            bail!("git.init: repository already exists");
        }
        dir.create_dir(".git")?;
        let result = (|| {
            dir.create_dir_all(".git/objects")?;
            dir.create_dir_all(".git/refs/heads")?;
            dir.write(".git/HEAD", format!("ref: refs/heads/{branch}\n"))?;
            dir.write(
                ".git/config",
                b"[core]\nrepositoryformatversion = 0\nbare = false\n",
            )?;
            Self::open(Arc::clone(&dir), cancelled, max_bytes).map(|mut snapshot| {
                snapshot.created = true;
                snapshot
            })
        })();
        if result.is_err() {
            let _ = dir.remove_dir_all(".git");
        }
        result
    }

    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        Ok(())
    }

    pub fn validate_reference_spelling(&self, name: &str) -> Result<()> {
        if name.is_empty()
            || Path::new(name)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("git: invalid reference path");
        }
        let mut directory = self.repo.git_dir().to_path_buf();
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
        let probe = tempfile::tempdir_in(self.temp.path())?;
        let mut directories = HashSet::new();
        let mut references = HashSet::new();
        for name in names {
            self.validate_reference_spelling(name)?;
            if !references.insert(name) {
                continue;
            }
            let path = Path::new(name);
            if let Some(parent) = path.parent() {
                create_scratch_directories(probe.path(), parent, &mut directories)?;
            }
            // Check the complete batch on the same filesystem before gix
            // creates any refs; two new destinations can alias each other.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(probe.path().join(path))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn publish(&self) -> Result<()> {
        self.publish_within(None).map(|_| ())
    }

    /// Publish, refusing first if the result would pass the VFS's size limit:
    /// what is staged, less what publication frees (see
    /// [`freed_bytes`](Self::freed_bytes)). The reservation is only that check;
    /// the caller counts the measured change once publication is over. Returns
    /// the bytes staged.
    pub fn publish_within(&self, quota: Option<&DiskQuota>) -> Result<u64> {
        self.check_cancelled()?;
        std::fs::write(self.temp.path().join(".git/config"), &self.original_config)?;
        let scratch =
            Dir::open_ambient_dir(self.temp.path().join(".git"), cap_std::ambient_authority())?;
        let files = read_files(&scratch, false, &self.cancelled, self.max_bytes)?;
        let worktree = self
            .pending_worktree
            .borrow()
            .as_ref()
            .map_or(0, files_bytes);
        let staged = files_bytes(&files).saturating_add(worktree);
        let growth = match quota {
            Some(quota) => {
                let growth = staged.saturating_sub(self.freed_bytes(quota)?);
                quota
                    .reserve(growth)
                    .map_err(|exceeded| range_error(format!("git: {exceeded}")))?;
                growth
            }
            None => 0,
        };
        let result = self.publish_files(&files);
        if let Some(quota) = quota {
            quota.release(growth);
        }
        result.map(|()| staged)
    }

    /// The bytes a publication frees: nothing for a new repository, the whole
    /// repository when it replaces the worktree, `.git` alone otherwise. A file a
    /// handle holds stays on disk once replaced, so it frees nothing.
    fn freed_bytes(&self, quota: &DiskQuota) -> Result<u64> {
        // A repository this operation created replaces nothing: its `.git` skeleton
        // is new too, and uncounted until publication.
        if self.created {
            return Ok(0);
        }
        let git_dir;
        let replaced = if self.pending_worktree.borrow().is_some() {
            &self.dir
        } else {
            git_dir = self.dir.open_dir(".git")?;
            &git_dir
        };
        let (total, held) =
            measure_with_held(replaced, &quota.held_files()).map_err(measure_error)?;
        let held_bytes = held
            .iter()
            .fold(0u64, |sum, (_, bytes)| sum.saturating_add(*bytes));
        Ok(total.saturating_sub(held_bytes))
    }

    fn publish_files(&self, files: &Files) -> Result<()> {
        let stage = format!(".git-submilli-{}", uuid::Uuid::new_v4());
        self.dir.create_dir(&stage)?;
        let result = self.publish_staged(&stage, files);
        if result.is_ok() {
            self.published.set(true);
            self.dir.remove_dir_all(&stage)?;
        } else {
            // If rollback itself failed, keep the old files for host recovery.
            let backup = self
                .dir
                .open_dir(format!("{stage}/old"))
                .and_then(|dir| dir.entries());
            let backup_absent_or_empty = match backup {
                Ok(mut entries) => entries.next().is_none(),
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            };
            if backup_absent_or_empty {
                let _ = self.dir.remove_dir_all(&stage);
            }
        }
        result
    }

    fn publish_staged(&self, stage: &str, metadata: &Files) -> Result<()> {
        let staging = self.dir.open_dir(stage)?;
        staging.create_dir_all("new/.git/objects")?;
        staging.create_dir_all("new/.git/refs/heads")?;
        write_files(&staging.open_dir("new/.git")?, metadata)?;
        staging.create_dir("old")?;
        let new = staging.open_dir("new")?;
        let old = staging.open_dir("old")?;
        let pending = self.pending_worktree.borrow();
        if let Some(files) = pending.as_ref() {
            write_files(&new, files)?;
        }
        self.check_cancelled()?;
        let names = self
            .dir
            .entries()?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let mut moved_old = Vec::new();
        let mut moved_new = Vec::new();
        // Once publication starts, finish or roll back before releasing the VFS even
        // if the caller was cancelled. No guest observes a partial checkout.
        let result = (|| -> Result<()> {
            for name in names {
                if name == stage {
                    continue;
                }
                if name != ".git" && pending.is_none() {
                    continue;
                }
                self.dir.rename(&name, &old, &name)?;
                moved_old.push(name);
            }
            for entry in new.entries()? {
                let name = entry?.file_name();
                new.rename(&name, &self.dir, &name)?;
                moved_new.push(name);
            }
            Ok(())
        })();
        if let Err(error) = result {
            for name in moved_new.iter().rev() {
                self.dir.rename(name, &new, name)?;
            }
            for name in moved_old.iter().rev() {
                old.rename(name, &self.dir, name)?;
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn worktree(&self) -> Result<Files> {
        read_files(&self.dir, true, &self.cancelled, self.max_bytes)
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
        if self.created && !self.published.get() {
            let _ = self.dir.remove_dir_all(".git");
        }
    }
}

/// A VFS git can't measure, too large or nested too deep, can't have a change
/// counted against its size limit: refused as a write past the limit is.
pub(super) fn measure_error(err: std::io::Error) -> wasmtime::Error {
    range_error(format!(
        "git: the VFS couldn't be measured against its size limit: {err}"
    ))
}

/// The bytes a set of files holds once written to the VFS.
fn files_bytes(files: &Files) -> u64 {
    files.values().fold(0u64, |total, (_, bytes)| {
        total.saturating_add(bytes.len() as u64)
    })
}

fn copy_metadata_to_scratch(
    scratch: &Path,
    files: &Files,
    max_bytes: u64,
    cancelled: &AtomicBool,
) -> Result<()> {
    let gitdir = scratch.join(".git");
    std::fs::create_dir(&gitdir)?;
    let mut directories = HashSet::new();
    create_scratch_directories(&gitdir, Path::new("objects/pack"), &mut directories)?;
    create_scratch_directories(&gitdir, Path::new("refs/heads"), &mut directories)?;
    for (path, (mode, bytes)) in files {
        validate_metadata_path(path)?;
        if *mode == 0o120000 {
            bail!("git: symlinks in repository metadata are unsupported");
        }
        if super::native_packs::is_pack_metadata(path) {
            continue;
        }
        if let Some(parent) = Path::new(path).parent() {
            create_scratch_directories(&gitdir, parent, &mut directories)?;
        }
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(gitdir.join(path))?;
        if path == "index" {
            let sanitized = super::index_limits::sanitize(bytes, max_bytes, cancelled)?;
            destination.write_all(&sanitized)?;
        } else {
            destination.write_all(bytes)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            destination.set_permissions(std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    super::native_packs::rebuild(files, &gitdir.join("objects/pack"), max_bytes, cancelled)
}

fn create_scratch_directories(
    root: &Path,
    path: &Path,
    directories: &mut HashSet<PathBuf>,
) -> Result<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        if directories.contains(&prefix) {
            continue;
        }
        // A different spelling must not reuse a directory already created on a
        // case-folding or Unicode-normalizing scratch filesystem.
        std::fs::create_dir(root.join(&prefix))?;
        directories.insert(prefix.clone());
    }
    Ok(())
}

fn open_isolated_repository(scratch: &Path, max_bytes: u64) -> Result<gix::Repository> {
    // Paths, helpers, includes, filters and external configuration are deliberately
    // absent. Remotes are parsed separately, without processing includes.
    std::fs::write(
        scratch.join(".git/config"),
        b"[core]\nrepositoryformatversion = 0\nbare = false\nfilemode = true\n",
    )?;
    Ok(gix::open_opts(
        scratch,
        gix::open::Options::isolated().config_overrides([
            &format!("gitoxide.objects.allocLimit={max_bytes}"),
            "core.logAllRefUpdates=false",
            "index.threads=1",
            "core.commitGraph=false",
            // Ignore server ACK IDs instead of letting them introduce
            // unvalidated local histories into the negotiation graph.
            "fetch.negotiationAlgorithm=noop",
        ]),
    )?)
}

fn validate_metadata_path(path: &str) -> Result<()> {
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

pub fn read_files(
    dir: &Dir,
    worktree: bool,
    cancelled: &AtomicBool,
    max_bytes: u64,
) -> Result<Files> {
    let mut state = WalkState {
        files: Files::new(),
        bytes: 0,
        paths: 0,
        cancelled,
        max_bytes,
    };
    walk(dir, "", worktree, &mut state, 0)?;
    Ok(state.files)
}

struct WalkState<'a> {
    files: Files,
    bytes: u64,
    paths: usize,
    cancelled: &'a AtomicBool,
    max_bytes: u64,
}

fn walk(
    dir: &Dir,
    prefix: &str,
    worktree: bool,
    state: &mut WalkState<'_>,
    depth: usize,
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
            )?;
            continue;
        }

        let (mode, content) = read_file_entry(
            dir,
            &name,
            &meta,
            state.max_bytes.saturating_sub(state.bytes),
        )?;
        state.bytes += content.len() as u64;
        if state.bytes > state.max_bytes {
            bail!("git: repository snapshot exceeds tenant Git memory limit");
        }
        state.files.insert(path, (mode, content));
    }
    Ok(())
}

fn read_file_entry(
    dir: &Dir,
    name: &str,
    meta: &cap_std::fs::Metadata,
    remaining_bytes: u64,
) -> Result<(u32, Vec<u8>)> {
    let entry = if meta.file_type().is_symlink() {
        (
            0o120000,
            dir.read_link_contents(name)?
                .into_os_string()
                .into_string()
                .map_err(|_| wasmtime::Error::msg("git: non-UTF-8 symlink target"))?
                .into_bytes(),
        )
    } else if meta.is_file() {
        #[cfg(unix)]
        {
            use cap_std::fs::MetadataExt;
            if meta.nlink() > 1 {
                bail!("git: hard-linked repository files are unsupported");
            }
        }
        let mut content = Vec::new();
        dir.open(name)?
            .take(remaining_bytes + 1)
            .read_to_end(&mut content)?;
        #[cfg(unix)]
        let executable = {
            use cap_std::fs::PermissionsExt;
            meta.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        (if executable { 0o100755 } else { 0o100644 }, content)
    } else {
        bail!("git: special files are unsupported");
    };
    Ok(entry)
}

pub fn write_files(dir: &Dir, files: &Files) -> Result<()> {
    // Install links last, so no ordinary file can be written through a link
    // whose spelling aliases a directory on the checkout filesystem.
    let ordinary = files.iter().filter(|(_, (mode, _))| *mode != 0o120000);
    let links = files.iter().filter(|(_, (mode, _))| *mode == 0o120000);
    for (path, (mode, bytes)) in ordinary.chain(links) {
        validate_path(path)?;
        prepare_parent(dir, Path::new(path))?;
        if *mode == 0o120000 {
            let target = std::str::from_utf8(bytes)?;
            #[cfg(unix)]
            dir.symlink_contents(target, path)?;
            #[cfg(not(unix))]
            {
                let _ = target;
                bail!("git: symlink checkout is unsupported on this platform");
            }
        } else {
            let mut file = dir.open_with(
                path,
                cap_std::fs::OpenOptions::new().write(true).create_new(true),
            )?;
            file.write_all(bytes)?;
            #[cfg(unix)]
            {
                use cap_std::fs::PermissionsExt;
                file.set_permissions(cap_std::fs::Permissions::from_mode(if *mode == 0o100755 {
                    0o755
                } else {
                    0o644
                }))?;
            }
        }
    }
    Ok(())
}

fn prepare_parent(dir: &Dir, path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
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
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
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
            let error = Snapshot::open(root.clone(), cancelled.clone(), 4096)
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
        let snapshot =
            Snapshot::init(vfs.dir().unwrap().clone(), "main", Default::default(), 4096).unwrap();
        let worktree: Files = [("notes.txt".to_string(), (0o644, vec![b'x'; 1000]))]
            .into_iter()
            .collect();
        *snapshot.pending_worktree.borrow_mut() = Some(worktree);
        let used = crate::runtime::measure_dir(&snapshot.dir).unwrap();
        let quota = crate::runtime::DiskQuota::new(used + 500, used);
        let error = snapshot.publish_within(Some(&quota)).unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
        assert!(
            error
                .downcast_ref::<crate::runtime::host::RangeError>()
                .is_some(),
            "a refusal is a RangeError the program can catch"
        );
        assert_eq!(quota.used(), used, "a refused publication claims nothing");

        // With room for the growth, the check passes and reserves nothing afterwards:
        // the caller counts what publication actually changed.
        let roomy = crate::runtime::DiskQuota::new(used + 2000, used);
        snapshot.publish_within(Some(&roomy)).unwrap();
        assert_eq!(roomy.used(), used);
        assert!(crate::runtime::measure_dir(&snapshot.dir).unwrap() >= used + 1000);
    }

    #[test]
    fn a_publication_that_replaces_more_than_it_writes_needs_no_room() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let root = vfs.dir().unwrap().clone();
        Snapshot::init(Arc::clone(&root), "main", Default::default(), 4096)
            .unwrap()
            .publish()
            .unwrap();
        root.write("old.txt", vec![b'o'; 5000]).unwrap();
        let snapshot = Snapshot::open(root, Default::default(), 4096).unwrap();
        let worktree: Files = [("new.txt".to_string(), (0o644, vec![b'n'; 1000]))]
            .into_iter()
            .collect();
        *snapshot.pending_worktree.borrow_mut() = Some(worktree);
        let used = crate::runtime::measure_dir(&snapshot.dir).unwrap();
        // Counting everything staged would need about 1 KB more than this allows.
        let tight = crate::runtime::DiskQuota::new(used + 10, used);
        snapshot.publish_within(Some(&tight)).unwrap();
    }

    #[test]
    fn a_new_repository_must_fit_the_size_limit() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot =
            Snapshot::init(vfs.dir().unwrap().clone(), "main", Default::default(), 4096).unwrap();
        let full = crate::runtime::DiskQuota::new(0, 0);
        let error = snapshot.publish_within(Some(&full)).unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
    }

    #[test]
    fn snapshot_bounds_configuration_event_expansion() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot =
            Snapshot::init(vfs.dir().unwrap().clone(), "main", Default::default(), 4096).unwrap();
        snapshot.publish().unwrap();
        snapshot
            .dir
            .write(".git/config", format!("[core]\n{}", "a\n".repeat(64)))
            .unwrap();
        let result = Snapshot::open(snapshot.dir.clone(), Default::default(), 4096);
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("configuration memory limit")
        );
        snapshot
            .dir
            .write(
                ".git/config",
                "[core]\nrepositoryformatversion = 0\nbare = false\n",
            )
            .unwrap();
        assert!(Snapshot::open(snapshot.dir.clone(), Default::default(), 4096).is_ok());
    }

    #[test]
    fn staging_does_not_write_through_aliased_symlinks() {
        for (link, directory) in [("A", "a"), ("é", "e\u{301}")] {
            for mode in [0o100644, 0o120000] {
                let temp = tempfile::tempdir().unwrap();
                let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
                dir.create_dir(".git").unwrap();
                dir.write(".git/HEAD", "original").unwrap();
                let files = Files::from([
                    (link.to_owned(), (0o120000, b".git".to_vec())),
                    (format!("{directory}/HEAD"), (mode, b"overwrite".to_vec())),
                ]);
                // Case-sensitive filesystems can keep both names. Aliasing
                // filesystems must fail without changing existing metadata.
                let _ = write_files(&dir, &files);
                assert_eq!(dir.read(".git/HEAD").unwrap(), b"original");
            }
        }
    }

    #[test]
    fn staging_rejects_existing_symlink_parents_and_file_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        dir.create_dir(".git").unwrap();
        dir.write(".git/HEAD", "original").unwrap();
        dir.symlink_contents(".git", "alias").unwrap();
        let files = Files::from([("alias/HEAD".to_owned(), (0o100644, b"overwrite".to_vec()))]);
        assert!(write_files(&dir, &files).is_err());
        assert_eq!(dir.read(".git/HEAD").unwrap(), b"original");
        dir.write("existing", "keep").unwrap();
        let files = Files::from([("existing".to_owned(), (0o100644, b"overwrite".to_vec()))]);
        assert!(write_files(&dir, &files).is_err());
        assert_eq!(dir.read("existing").unwrap(), b"keep");
    }
}

#[cfg(test)]
mod scratch_copy_tests {
    use super::*;

    #[test]
    fn reference_updates_reject_existing_and_prospective_filesystem_aliases() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot =
            Snapshot::init(vfs.dir().unwrap().clone(), "main", Default::default(), 4096).unwrap();
        for (first, second) in [("Main", "main"), ("é", "e\u{301}")] {
            let probe = tempfile::tempdir_in(snapshot.temp.path()).unwrap();
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
                let original = snapshot.repo.git_dir().join(&paths[0]);
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
        let snapshot =
            Snapshot::init(vfs.dir().unwrap().clone(), "main", Default::default(), 4096).unwrap();
        snapshot.publish().unwrap();
        // A case-folding mount cannot hold this independent untracked directory.
        if snapshot.dir.try_exists(".GIT").unwrap() {
            return;
        }
        snapshot.dir.create_dir(".GIT").unwrap();
        snapshot.dir.write(".GIT/keep", "untracked data").unwrap();
        let before = snapshot.dir.read(".git/HEAD").unwrap();
        let error =
            super::super::operations::replace_worktree(&snapshot, &Files::new()).unwrap_err();
        assert!(error.to_string().contains("noncanonical worktree metadata"));
        assert_eq!(snapshot.dir.read(".GIT/keep").unwrap(), b"untracked data");
        assert_eq!(snapshot.dir.read(".git/HEAD").unwrap(), before);
        assert!(snapshot.pending_worktree.borrow().is_none());
    }

    #[test]
    fn metadata_copy_preserves_distinct_refs_or_rejects_filesystem_aliases() {
        for (first, second) in [("Main", "main"), ("é", "e\u{301}")] {
            for directories in [false, true] {
                let scratch = tempfile::tempdir().unwrap();
                let probe = scratch.path().join("probe");
                std::fs::create_dir(&probe).unwrap();
                std::fs::write(probe.join(first), "probe").unwrap();
                let aliases = probe.join(second).exists();
                let paths = if directories {
                    [
                        format!("refs/heads/{first}/one"),
                        format!("refs/heads/{second}/two"),
                    ]
                } else {
                    [
                        format!("refs/heads/{first}"),
                        format!("refs/heads/{second}"),
                    ]
                };
                let files = Files::from([
                    (paths[0].clone(), (0o100644, b"first ref".to_vec())),
                    (paths[1].clone(), (0o100644, b"second ref".to_vec())),
                ]);
                let original = files.clone();
                let result =
                    copy_metadata_to_scratch(scratch.path(), &files, 4096, &AtomicBool::new(false));
                assert_eq!(files, original);
                if aliases {
                    assert!(result.is_err(), "accepted aliases: {paths:?}");
                    let (path, (_, bytes)) = files.first_key_value().unwrap();
                    assert_eq!(
                        std::fs::read(scratch.path().join(".git").join(path)).unwrap(),
                        *bytes
                    );
                } else {
                    result.unwrap();
                    for (path, (_, bytes)) in &files {
                        assert_eq!(
                            std::fs::read(scratch.path().join(".git").join(path)).unwrap(),
                            *bytes
                        );
                    }
                }
            }
        }
    }
}
