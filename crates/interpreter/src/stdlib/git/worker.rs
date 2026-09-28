//! Repository jobs run on the blocking pool and retain their VFS through cleanup.
use super::{Job, operations, storage, transport};
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
    let root = vfs
        .dir()
        .ok_or_else(|| wasmtime::Error::msg("git: VFS is disabled"))?;
    let branch = requested_branch(op, args)?;
    authorize_operation(job, op, args, branch)?;
    let mut snapshot = open_snapshot(root, job, op, branch)?;
    snapshot.remotes()?;
    let mut changed = op == "init";
    let result = match op {
        "open" | "init" => Value::Null,
        "clone" => {
            clone_repository(&mut snapshot, job, operations::text_arg(args, 0)?, branch)?;
            changed = true;
            Value::Null
        }
        "add" => {
            let paths = serde_json::from_value::<Vec<String>>(args[0].clone())?;
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

fn open_snapshot(
    root: &cap_std::fs::Dir,
    job: &Job,
    op: &str,
    branch: &str,
) -> Result<storage::Snapshot> {
    let relative = job.path.trim_start_matches('/');
    let relative = if relative.is_empty() { "." } else { relative };
    let create = op == "init" || op == "clone";
    let mut prefix = std::path::PathBuf::new();
    for component in std::path::Path::new(relative).components() {
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
    if canonical != std::path::Path::new(relative)
        && !(relative == "." && canonical.as_os_str().is_empty())
    {
        bail!("git: repository paths must not contain symlinks; use the canonical VFS path");
    }
    let dir = Arc::new(root.open_dir(relative)?);
    if op == "clone" && dir.entries()?.next().is_some() {
        bail!("git.clone: destination must be empty");
    }
    if matches!(
        op,
        "add"
            | "commit"
            | "createBranch"
            | "switchBranch"
            | "addRemote"
            | "setRemoteUrl"
            | "fetch"
            | "pull"
    ) {
        storage::reject_in_progress(&dir)?;
    }
    let snapshot = if create {
        storage::Snapshot::init(
            dir,
            if branch.is_empty() { "main" } else { branch },
            job.cancelled.clone(),
            job.max_bytes,
        )?
    } else {
        storage::Snapshot::open(dir, job.cancelled.clone(), job.max_bytes)?
    };
    Ok(snapshot)
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
    std::fs::write(
        snapshot.repo.git_dir().join("HEAD"),
        format!("ref: refs/heads/{branch}\n"),
    )?;
    Ok(())
}
