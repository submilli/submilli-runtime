//! Repository jobs run on the blocking pool and retain their VFS through cleanup.
use super::{Job, location::Location, lock::RepositoryLock, operations, storage, transport};

use crate::runtime::fs::{ContainError, check_repository_clear_of_mounts};
use crate::runtime::host::permission_denied_read_only;
use crate::runtime::vfs::Access;
use cap_std::fs::Dir;
use serde_json::{Value, json};
use std::sync::Arc;
use wasmtime::{Result, bail};

pub(super) enum Output {
    Json(Value),
    Bytes(Vec<u8>),
}

pub(super) fn run(
    vfs: &crate::runtime::Vfs,
    job: &Job,
    op: &str,
    args: &[Value],
) -> Result<Output> {
    let (root, relative, placement) = locate_repository(vfs, job)?;
    let branch = requested_branch(op, args)?;
    authorize_operation(job, op, args, branch)?;
    let creates = matches!(op, "init" | "clone");
    // A change stages inside the repository, so a read-only volume refuses it
    // before anything is written.
    if writes(op) {
        refuse_read_only_repository(job, op, &placement)?;
        job.algorithm_fuel.before_effect();
    }
    // Declared first, the permit is released last, after the snapshot's cleanup.
    let (_permit, mut snapshot) =
        open_snapshot(&root, &relative, &placement, job, op, creates, branch)?;
    let mut changed = op == "init";
    let result = match op {
        "open" | "init" => Value::Null,
        "clone" => {
            clone_repository(&mut snapshot, job, operations::text_arg(args, 0)?, branch)?;
            changed = true;
            Value::Null
        }
        "add" => {
            let paths = args.first().ok_or_else(|| {
                crate::runtime::host::fatal_host_error("git.add worker is missing decoded paths")
            })?;
            let paths = serde_json::from_value::<Vec<String>>(paths.clone())?;
            operations::add(&snapshot, &paths)?;
            changed = true;
            Value::Null
        }
        "commit" => {
            job.check(
                "git.commit",
                json!({"path":job.path,"branch":operations::current_branch(&snapshot)?}),
            )?;
            let id = operations::commit(&snapshot, operations::text_arg(args, 0)?, &job.config)?;
            changed = true;
            json!(id)
        }
        "createBranch" => {
            operations::create_branch(
                &snapshot,
                operations::text_arg(args, 0)?,
                operations::text_arg(args, 1)?,
            )?;
            changed = true;
            Value::Null
        }
        "switchBranch" => {
            operations::checkout(&snapshot, operations::text_arg(args, 0)?)?;
            changed = true;
            Value::Null
        }
        "addRemote" | "setRemoteUrl" => {
            operations::set_remote(
                &mut snapshot,
                operations::text_arg(args, 0)?,
                operations::text_arg(args, 1)?,
                op == "addRemote",
            )?;
            changed = true;
            Value::Null
        }
        "fetch" => {
            let branches = transport::fetch(
                &snapshot,
                job,
                operations::text_arg(args, 0)?,
                operations::text_arg(args, 1)?,
            )?;
            changed = true;
            json!({"branches":branches.branches})
        }
        "pull" => {
            let value = transport::pull(
                &snapshot,
                job,
                operations::text_arg(args, 0)?,
                operations::text_arg(args, 1)?,
            )?;
            changed = true;
            value
        }
        "show" => {
            return Ok(Output::Bytes(operations::show(
                &snapshot,
                operations::text_arg(args, 0)?,
                operations::text_arg(args, 1)?,
            )?));
        }
        _ => operations::read(&snapshot, op, args)?,
    };
    if changed {
        snapshot.publish()?;
    }
    Ok(Output::Json(result))
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

fn requested_branch<'a>(op: &str, args: &'a [Value]) -> Result<&'a str> {
    let branch = args
        .last()
        .and_then(|o| o.get("branch"))
        .and_then(Value::as_str)
        .unwrap_or(if op == "clone" { "" } else { "main" });
    if let Some(explicit) = args
        .last()
        .and_then(|options| options.get("branch"))
        .and_then(Value::as_str)
    {
        storage::validate_branch(explicit)?;
    }
    Ok(branch)
}

fn authorize_operation(job: &Job, op: &str, args: &[Value], branch: &str) -> Result<()> {
    match op {
        "init" => job.check("git.init", json!({"path":job.path})),
        "clone" => {
            let url = transport::canonical_url(operations::text_arg(args, 0)?)?;
            job.check(
                "git.clone",
                json!({"path":job.path,"remote":url,"remoteName":"origin","branch":branch}),
            )
        }
        _ => Ok(()),
    }
}

/// `relative` is the repository's path within the volume `root` holds, `.` for
/// the volume's own root. The repository is held first, then one of the
/// workers Git shares, so calls waiting on a busy repository don't keep
/// others from working.
fn open_snapshot(
    root: &cap_std::fs::Dir,
    relative: &std::path::Path,
    placement: &crate::runtime::vfs::Placement,
    job: &Job,
    op: &str,
    create: bool,
    branch: &str,
) -> Result<(tokio::sync::OwnedSemaphorePermit, storage::Snapshot)> {
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
    if create {
        root.create_dir_all(relative)?;
    }
    let canonical = root.canonicalize(relative)?;
    if canonical != relative
        && !(relative == std::path::Path::new(".") && canonical.as_os_str().is_empty())
    {
        bail!("git: repository paths must not contain symlinks; use the canonical VFS path");
    }
    let dir = Arc::new(root.open_dir(relative)?);
    let location = Location::new(Arc::clone(&dir), placement, relative)?;
    if op == "clone" && dir.entries()?.next().is_some() {
        bail!("git.clone: destination must be empty");
    }
    if writes(op) && !create {
        storage::reject_in_progress(&dir)?;
    }
    let lock = RepositoryLock::acquire(location.identity, &job.cancelled)?;
    let permit = job.worker_permit()?;
    let opening = storage::Opening {
        cancelled: job.cancelled.clone(),
        max_bytes: job.max_bytes,
        quota: placement.quota().cloned(),
        meter: Arc::clone(&job.meter),
    };
    let mut snapshot = if create {
        storage::Snapshot::init(
            &location,
            lock,
            if branch.is_empty() { "main" } else { branch },
            opening,
        )?
    } else {
        storage::Snapshot::open(&location, lock, opening, writes(op))?
    };
    snapshot.algorithm_fuel = Arc::clone(&job.algorithm_fuel);
    Ok((permit, snapshot))
}

/// Whether `op` changes the repository, and so needs a stage.
fn writes(op: &str) -> bool {
    matches!(
        op,
        "init"
            | "clone"
            | "add"
            | "commit"
            | "createBranch"
            | "switchBranch"
            | "addRemote"
            | "setRemoteUrl"
            | "fetch"
            | "pull"
    )
}

fn clone_repository(
    snapshot: &mut storage::Snapshot,
    job: &Job,
    url: &str,
    branch: &str,
) -> Result<()> {
    operations::set_remote(snapshot, "origin", url, true)?;
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
