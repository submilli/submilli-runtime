//! Repository jobs run on the blocking pool and retain their VFS through cleanup.
//! Acquire each repository lock before a worker permit, so calls waiting for one
//! busy repository do not occupy workers needed by other repositories.
use super::{Job, location::Location, lock::RepositoryLock, operations, storage, transport};

use crate::runtime::fs::{ContainError, check_repository_clear_of_mounts};
use crate::runtime::host::permission_denied_read_only;
use crate::runtime::vfs::Access;
use cap_std::fs::Dir;
use serde_json::{Value, json};
use std::sync::Arc;
use wasmtime::{Result, bail};

pub(super) fn open_repository(
    vfs: &crate::runtime::Vfs,
    job: &Job,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
    let (root, relative, placement) = locate_repository(vfs, job)?;
    reject_symlink_components(&root, &relative)?;
    let location = open_directory(&root, &relative, &placement)?;
    let lock = RepositoryLock::acquire(location.identity, &job.cancelled)?;
    let permit = job.worker_permit()?;
    let mut snapshot = storage::Snapshot::open(&location, lock, opening(job, &placement), false)?;
    snapshot.algorithm_fuel = Arc::clone(&job.algorithm_fuel);
    snapshot.history_cache = job.history_cache.clone();
    Ok((permit, snapshot))
}

pub(super) fn open_repository_for_write(
    vfs: &crate::runtime::Vfs,
    job: &Job,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
    let (root, relative, placement) = locate_repository(vfs, job)?;
    refuse_read_only_repository(job, &job.op, &placement)?;
    reject_symlink_components(&root, &relative)?;
    let location = open_directory(&root, &relative, &placement)?;
    storage::reject_in_progress(&location.dir)?;
    let lock = RepositoryLock::acquire(location.identity, &job.cancelled)?;
    let permit = job.worker_permit()?;
    let mut snapshot = storage::Snapshot::open(&location, lock, opening(job, &placement), true)?;
    snapshot.algorithm_fuel = Arc::clone(&job.algorithm_fuel);
    snapshot.history_cache = job.history_cache.clone();
    Ok((permit, snapshot))
}

pub(super) fn create_repository(
    vfs: &crate::runtime::Vfs,
    job: &Job,
    branch: &str,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
    let (root, relative, placement) = locate_repository(vfs, job)?;
    refuse_read_only_repository(job, &job.op, &placement)?;
    job.algorithm_fuel.before_effect();
    reject_symlink_components(&root, &relative)?;
    root.create_dir_all(&relative)?;
    let location = open_directory(&root, &relative, &placement)?;
    initialize_repository(&location, &placement, job, branch)
}

pub(super) fn create_clone_repository(
    vfs: &crate::runtime::Vfs,
    job: &Job,
    branch: &str,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
    let (root, relative, placement) = locate_repository(vfs, job)?;
    refuse_read_only_repository(job, &job.op, &placement)?;
    job.algorithm_fuel.before_effect();
    reject_symlink_components(&root, &relative)?;
    root.create_dir_all(&relative)?;
    let location = open_directory(&root, &relative, &placement)?;
    if location.dir.entries()?.next().is_some() {
        bail!("git.clone: destination must be empty");
    }
    initialize_repository(&location, &placement, job, branch)
}

fn initialize_repository(
    location: &Location,
    placement: &crate::runtime::vfs::Placement,
    job: &Job,
    branch: &str,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
    let lock = RepositoryLock::acquire(location.identity, &job.cancelled)?;
    let permit = job.worker_permit()?;
    let mut snapshot = storage::Snapshot::init(
        location,
        lock,
        if branch.is_empty() { "main" } else { branch },
        opening(job, placement),
    )?;
    snapshot.algorithm_fuel = Arc::clone(&job.algorithm_fuel);
    snapshot.history_cache = job.history_cache.clone();
    Ok((permit, snapshot))
}

fn opening(job: &Job, placement: &crate::runtime::vfs::Placement) -> storage::Opening {
    storage::Opening {
        cancelled: job.cancelled.clone(),
        max_bytes: job.max_bytes,
        quota: placement.quota().cloned(),
        meter: Arc::clone(&job.meter),
    }
}

/// The volume holding the repository at `job.path`, the repository's path within
/// it, and the volume's placement. A repository that overlaps a mount point — one
/// below it, or reaching into it through an alias — is refused; see
/// [`check_repository_clear_of_mounts`].
fn locate_repository(
    vfs: &crate::runtime::Vfs,
    job: &Job,
) -> Result<(Arc<Dir>, std::path::PathBuf, crate::runtime::vfs::Placement)> {
    let relative = std::path::Path::new(job.path.trim_start_matches('/'));
    let relative = if relative.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        relative
    };
    let (root, relative, placement) = vfs
        .locate(relative)
        .ok_or_else(|| wasmtime::Error::msg("git: VFS is disabled"))?;
    match check_repository_clear_of_mounts(&root, &relative, &placement) {
        Ok(()) => {}
        Err(ContainError::MountPoint(mount)) => bail!(
            "git: repository {} overlaps the mount point {mount}; use a directory that \
             holds no mount point, and spell a path inside the mount exactly as {mount}/…",
            job.path
        ),
        Err(err) => bail!("git: repository {}: {err}", job.path),
    }
    Ok((root, relative, placement))
}

/// Refuse a change to a repository in a volume mounted read-only.
fn refuse_read_only_repository(
    job: &Job,
    op: &str,
    placement: &crate::runtime::vfs::Placement,
) -> Result<()> {
    if placement.access() == Access::ReadWrite {
        return Ok(());
    }
    Err(permission_denied_read_only(
        &job.caller,
        format!("git.{op}"),
        format!(
            "repository {} is in the volume mounted read-only at {}",
            job.path,
            placement.mount_point()
        ),
    ))
}

pub(super) fn requested_branch<'a>(options: &'a Value, default: &'a str) -> Result<&'a str> {
    validate_branch_options(options)?;
    Ok(options
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or(default))
}

pub(super) fn validate_branch_options(options: &Value) -> Result<()> {
    if let Some(branch) = options.get("branch").and_then(Value::as_str) {
        storage::validate_branch(branch)?;
    }
    Ok(())
}

fn reject_symlink_components(root: &Dir, relative: &std::path::Path) -> Result<()> {
    let mut prefix = std::path::PathBuf::new();
    for component in relative.components() {
        prefix.push(component);
        match root.symlink_metadata(&prefix) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("git: repository path contains a symlink")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn open_directory(
    root: &Dir,
    relative: &std::path::Path,
    placement: &crate::runtime::vfs::Placement,
) -> Result<Location> {
    let canonical = root.canonicalize(relative)?;
    if canonical != relative
        && !(relative == std::path::Path::new(".") && canonical.as_os_str().is_empty())
    {
        bail!("git: repository paths must not contain symlinks; use the canonical VFS path");
    }
    let dir = Arc::new(root.open_dir(relative)?);
    Location::new(dir, placement, relative)
}

pub(super) fn clone_repository(
    snapshot: &mut storage::Snapshot,
    job: &Job,
    url: &str,
    branch: &str,
) -> Result<()> {
    operations::add_remote(snapshot, "origin", url)?;
    let fetched = transport::fetch(snapshot, job, "origin", branch)?;
    let branch = if branch.is_empty() {
        fetched.default_branch.as_deref().ok_or_else(|| {
            wasmtime::Error::msg(
                "git.clone: remote HEAD does not identify a branch; specify options.branch",
            )
        })?
    } else {
        branch
    };
    job.check(
        "git.clone",
        json!({"path":job.path,"remote":transport::canonical_url(url)?,"remoteName":"origin","branch":branch}),
    )?;
    let next = operations::resolve_tree(snapshot, &format!("refs/remotes/origin/{branch}"))?;
    operations::replace_worktree(snapshot, &next)?;
    operations::create_branch(snapshot, branch, &format!("refs/remotes/origin/{branch}"))?;
    snapshot.write_head(branch)
}

#[cfg(test)]
pub(super) fn run(vfs: &crate::runtime::Vfs, job: &Job, op: &str, args: &[Value]) -> Result<()> {
    let text = |index| operations::text_arg(args, index).map(str::to_owned);
    match op {
        "open" => super::open(vfs, job, ()),
        "init" => super::init(vfs, job, args.last().cloned().unwrap_or(Value::Null)),
        "clone" => super::clone_repository(
            vfs,
            job,
            super::CloneArguments {
                url: text(0)?,
                options: args.last().cloned().unwrap_or(Value::Null),
            },
        ),
        "status" => super::status(vfs, job, ()).map(|_| ()),
        "branches" => super::branches(vfs, job, ()).map(|_| ()),
        "remotes" => super::remotes(vfs, job, ()).map(|_| ()),
        "log" => super::log(vfs, job, args.first().cloned().unwrap_or(Value::Null)).map(|_| ()),
        "diff" => super::diff(vfs, job, args.first().cloned().unwrap_or(Value::Null)).map(|_| ()),
        "show" => super::show(
            vfs,
            job,
            super::ShowArguments {
                revision: text(0)?,
                path: text(1)?,
            },
        )
        .map(|_| ()),
        "add" => {
            let paths = args.first().ok_or_else(|| {
                crate::runtime::host::fatal_host_error("git.add worker is missing decoded paths")
            })?;
            super::add(vfs, job, serde_json::from_value(paths.clone())?)
        }
        "commit" => super::commit(vfs, job, text(0)?).map(|_| ()),
        "createBranch" => super::create_branch(
            vfs,
            job,
            super::BranchArguments {
                name: text(0)?,
                start: text(1)?,
            },
        ),
        "switchBranch" => super::switch_branch(vfs, job, text(0)?),
        "addRemote" => super::add_remote(
            vfs,
            job,
            super::RemoteArguments {
                name: text(0)?,
                url: text(1)?,
            },
        ),
        "setRemoteUrl" => super::set_remote_url(
            vfs,
            job,
            super::RemoteArguments {
                name: text(0)?,
                url: text(1)?,
            },
        ),
        "fetch" => super::fetch(
            vfs,
            job,
            super::FetchArguments {
                remote: text(0)?,
                branch: text(1)?,
            },
        )
        .map(|_| ()),
        "pull" => super::pull(
            vfs,
            job,
            super::FetchArguments {
                remote: text(0)?,
                branch: text(1)?,
            },
        )
        .map(|_| ()),
        _ => panic!("unknown test operation {op}"),
    }
}
