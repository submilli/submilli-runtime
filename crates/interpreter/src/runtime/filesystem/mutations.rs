//! Native tree mutations with interpreter-supplied resource accounting.
use super::MutationBudget;
use crate::runtime::{
    DiskQuota, QuotaCharge, Vfs,
    fs::{self, ContainError, LinkPath, MAX_REMOVE_ENTRIES},
};
use crate::stdlib::shared::{contain_trap, quota_refusal, write_target_trap};
use std::sync::Arc;
pub(super) fn remove(
    vfs: &Vfs,
    path: &str,
    recursive: bool,
    budget: &mut dyn MutationBudget,
) -> wasmtime::Result<()> {
    let resolved =
        fs::resolve_link(vfs, vfs.cwd(), path).map_err(|e| contain_trap("fs.remove", path, &e))?;
    let context = MutationContext {
        op: "fs.remove",
        path,
        quota: resolved.placement().quota().cloned(),
        changed: false,
    };
    meter_mutation(budget, context, |step| {
        fs::removal::remove(&resolved, recursive, step)
    })
}
pub(super) fn move_entry(
    vfs: &Vfs,
    from: &str,
    to: &str,
    budget: &mut dyn MutationBudget,
) -> wasmtime::Result<()> {
    let from_resolved =
        fs::resolve_link(vfs, vfs.cwd(), from).map_err(|e| contain_trap("fs.move", from, &e))?;
    let to_resolved =
        fs::resolve_link(vfs, vfs.cwd(), to).map_err(|e| contain_trap("fs.move", to, &e))?;
    let pair = format!("{from} -> {to}");
    if from_resolved.is_root() || to_resolved.is_root() {
        wasmtime::bail!(
            "fs.move {pair}: the VFS root and mount points cannot be moved or replaced"
        );
    }
    if from_resolved
        .same_entry(&to_resolved)
        .map_err(|e| contain_trap("fs.move", &pair, &e))?
    {
        return Ok(());
    }
    if !from_resolved
        .placement()
        .same_volume(to_resolved.placement())
    {
        let mut work = CopyWork::default();
        let result = move_across(budget, &from_resolved, &to_resolved, &pair, &mut work);
        work.settle(budget)?;
        return result;
    }
    // A file moved over another frees the one it replaced; a move onto the same
    // file, under any spelling, frees nothing.
    let moved = from_resolved.regular_file().map(|(file, _)| file);
    let replaced = to_resolved
        .regular_file()
        .filter(|(file, _)| Some(*file) != moved);
    let context = MutationContext {
        op: "fs.move",
        path: &pair,
        quota: None,
        changed: false,
    };
    meter_mutation(budget, context, |step| {
        crate::runtime::fs::removal::rename(&from_resolved, &to_resolved, step)
    })?;
    if let (Some(quota), Some((file, bytes))) = (to_resolved.placement().quota(), replaced) {
        quota.release_file(file, bytes);
    }
    Ok(())
}
pub(super) fn copy(
    vfs: &Vfs,
    from: &str,
    to: &str,
    recursive: bool,
    budget: &mut dyn MutationBudget,
) -> wasmtime::Result<()> {
    let from_resolved =
        fs::resolve_link(vfs, vfs.cwd(), from).map_err(|e| contain_trap("fs.copy", from, &e))?;
    let pair = format!("{from} -> {to}");
    let mount = from_resolved
        .contains_mount_point()
        .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
    if let Some(mount) = mount {
        wasmtime::bail!(
            "fs.copy {pair}: {from} contains the mount point {mount}; copy the \
                     mount's contents separately"
        );
    }
    // The content path routes to the same volume as the link path, and resolves
    // even when the destination's parent does not exist yet.
    let to_volume =
        fs::resolve_content(vfs, vfs.cwd(), to).map_err(|e| contain_trap("fs.copy", to, &e))?;
    from_resolved
        .check_copy_destination(&to_volume)
        .map_err(|e| contain_trap("fs.copy", &pair, &e))?;
    // A recursive copy used to reach `create_dir_all(to)`, which built the whole
    // destination chain. Resolving the destination now opens its parent, so that
    // chain has to exist first or a working call starts failing.
    if recursive
        && from_resolved
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir())
    {
        to_volume
            .create_dir_all()
            .map_err(|e| write_target_trap("fs.copy", to, &e))?;
    }
    let to_resolved =
        fs::resolve_link(vfs, vfs.cwd(), to).map_err(|e| contain_trap("fs.copy", to, &e))?;
    let quota = to_resolved.placement().quota().cloned();
    let mut work = CopyWork::default();
    let result = copy_bounded(
        &from_resolved,
        &to_resolved,
        &CopyRun {
            recursive,
            links: LinkCopies::BestEffort,
            op: "fs.copy",
            pair: &pair,
            quota: quota.as_ref(),
        },
        &mut work,
    );
    work.settle(budget)?;
    result
}
/// Reproduce a symlink at `to` with `from`'s target stored verbatim.
///
/// Verbatim is the point: the target is never resolved, so a link that escapes the root is
/// reproduced as one that still refuses rather than as a working door out of it.
fn copy_link(from: &LinkPath, to: &LinkPath) -> Result<(), ContainError> {
    to.symlink(&from.read_link_contents()?, from.link_kind()?)
}

/// A move between two volumes, which no rename can make: copy into a temporary
/// sibling of the destination (charged to the destination's limit), rename it into
/// place, then remove the source (released from the source's limit).
///
/// Until the rename, a failure removes the temporary copy and leaves the source
/// untouched. Once the destination is in place it is kept; if the source then
/// cannot be removed, the call fails saying both copies may exist. A link the copy
/// cannot reproduce fails the move, since the source is removed after.
fn move_across(
    budget: &mut dyn MutationBudget,
    from: &LinkPath,
    to: &LinkPath,
    pair: &str,
    work: &mut CopyWork,
) -> wasmtime::Result<()> {
    let meta = from
        .symlink_metadata()
        .map_err(|e| contain_trap("fs.move", pair, &e))?;
    let is_dir = meta.is_dir();
    // The source is removed last, so a tree too deep or too large to remove, or
    // one holding a mount point, is refused now, before a copy of it lands in the
    // destination. The destination gets the checks its rename will run, so a
    // move that must fail does no copying first.
    meter_mutation(
        budget,
        MutationContext {
            op: "fs.move",
            path: pair,
            quota: None,
            changed: false,
        },
        |step| {
            crate::runtime::fs::removal::validate(from, &mut *step)?;
            crate::runtime::fs::removal::validate(to, step)
        },
    )?;
    refuse_unfitting_destination(to, is_dir, pair)?;
    let to_quota = to.placement().quota().cloned();
    let staged = to.temp_sibling();
    let run = CopyRun {
        recursive: true,
        links: LinkCopies::Required,
        op: "fs.move",
        pair,
        quota: to_quota.as_ref(),
    };
    let copied = copy_bounded(from, &staged, &run, work).and_then(|()| {
        let replaced = to.regular_file();
        meter_mutation(
            budget,
            MutationContext {
                op: "fs.move",
                path: pair,
                quota: None,
                changed: true,
            },
            |step| crate::runtime::fs::removal::rename(&staged, to, step),
        )?;
        if let (Some(quota), Some((file, bytes))) = (to_quota.as_ref(), replaced) {
            quota.release_file(file, bytes);
        }
        Ok(())
    });
    if let Err(err) = copied {
        // Best effort: what cannot be removed stays charged, so the count errs
        // toward refusing a later write.
        let _ = meter_mutation(
            budget,
            MutationContext {
                op: "fs.move",
                path: pair,
                quota: to_quota,
                changed: true,
            },
            |step| crate::runtime::fs::removal::remove(&staged, true, step),
        );
        return Err(err);
    }
    meter_mutation(
        budget,
        MutationContext {
            op: "fs.move",
            path: pair,
            quota: from.placement().quota().cloned(),
            changed: true,
        },
        |step| {
            crate::runtime::fs::removal::remove(from, true, step).map_err(|error| {
            if crate::runtime::host::ends_the_run(&error) { return error; }
            wasmtime::Error::msg(format!(
                "fs.move {pair}: moved to the destination but could not remove the source, so both may now exist: {error}"
            ))
        })
        },
    )
}

/// Refuse a destination a rename of the source could not replace — a directory
/// in place of a file, a file in place of a directory, or a directory that isn't
/// empty — as a rename within one volume would, before any copying.
fn refuse_unfitting_destination(
    to: &LinkPath,
    moving_dir: bool,
    pair: &str,
) -> wasmtime::Result<()> {
    let existing = match to.symlink_metadata() {
        Ok(existing) => existing,
        Err(ContainError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(err) => return Err(contain_trap("fs.move", pair, &err)),
    };
    let replacing_dir = existing.is_dir();
    if replacing_dir != moving_dir {
        let (what, onto) = if moving_dir {
            ("a directory", "something that is not a directory")
        } else {
            ("a file", "a directory")
        };
        wasmtime::bail!("fs.move {pair}: cannot move {what} onto {onto}");
    }
    if !replacing_dir {
        return Ok(());
    }
    let has_entries = to
        .entries()
        .map_err(|e| contain_trap("fs.move", pair, &e))?
        .next()
        .is_some();
    if has_entries {
        wasmtime::bail!("fs.move {pair}: the destination directory is not empty");
    }
    Ok(())
}

struct MutationContext<'a> {
    op: &'static str,
    path: &'a str,
    quota: Option<Arc<DiskQuota>>,
    changed: bool,
}

fn meter_mutation(
    budget: &mut dyn MutationBudget,
    context: MutationContext<'_>,
    run: impl FnOnce(
        &mut dyn FnMut(crate::runtime::fs::removal::Step) -> wasmtime::Result<()>,
    ) -> wasmtime::Result<()>,
) -> wasmtime::Result<()> {
    use crate::runtime::fs::removal::Step;
    budget.begin()?;
    let mut changed = context.changed;
    let result = run(&mut |step| {
        match step {
            Step::Inspect(units) | Step::Mutate(units) if changed => {
                budget.syscalls(units, true)?;
            }
            Step::Inspect(units) | Step::Mutate(units) => {
                budget.syscalls(units, false)?;
            }
            Step::Reserve(bytes) => budget.reserve(bytes)?,
            Step::Unlinked(file) => {
                changed = true;
                budget.effect();
                if let (Some(quota), Some((file, bytes))) = (&context.quota, file) {
                    quota.release_file(file, bytes);
                }
            }
        }
        Ok(())
    });
    budget.end();
    result.map_err(|error| match error.downcast::<ContainError>() {
        Ok(error) => contain_trap(context.op, context.path, &error),
        Err(error) => error,
    })
}

/// How one copy walk treats its tree.
#[derive(Clone, Copy)]
pub(crate) struct CopyRun<'a> {
    pub(crate) recursive: bool,
    pub(crate) links: LinkCopies,
    /// `fs.copy` or `fs.move`, for diagnostics.
    pub(crate) op: &'a str,
    /// `from -> to` as the program wrote them, for diagnostics.
    pub(crate) pair: &'a str,
    /// The destination volume's limit.
    pub(crate) quota: Option<&'a Arc<DiskQuota>>,
}

/// What a copy does with a link inside the tree that the platform will not
/// reproduce, such as an absolute one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkCopies {
    /// Skip it: a copy leaves the source in place, so nothing is lost.
    BestEffort,
    /// Fail: a move removes the source afterwards, which would lose the link.
    Required,
}

/// Matches the entry-count limit used by removal.
pub(crate) const MAX_COPY_ENTRIES: u64 = MAX_REMOVE_ENTRIES as u64;
pub(crate) const MAX_COPY_DEPTH: usize = 64;

#[derive(Default)]
pub(crate) struct CopyWork {
    pub(crate) entries: u64,
    pub(crate) bytes: u64,
}

impl CopyWork {
    fn visit(&mut self, op: &str, pair: &str) -> wasmtime::Result<()> {
        if self.entries >= MAX_COPY_ENTRIES {
            return Err(crate::runtime::host::range_error(format!(
                "{op} {pair}: exceeds {MAX_COPY_ENTRIES} entries; copy smaller subtrees"
            )));
        }
        self.entries += 1;
        Ok(())
    }

    fn settle(&self, budget: &mut dyn MutationBudget) -> wasmtime::Result<()> {
        budget.copied(self.entries, self.bytes)
    }
}

pub(crate) struct CopyFrame {
    from: LinkPath,
    to: LinkPath,
    from_dir: Arc<cap_std::fs::Dir>,
    to_dir: Arc<cap_std::fs::Dir>,
    entries: cap_std::fs::ReadDir,
}

/// Depth-first traversal retains directory iterators instead of recursing or
/// collecting every sibling path. Symlinks are copied as links, never followed.
pub(crate) fn copy_bounded(
    from: &LinkPath,
    to: &LinkPath,
    run: &CopyRun<'_>,
    work: &mut CopyWork,
) -> wasmtime::Result<()> {
    let mut frames = Vec::new();
    if let Some(frame) = copy_entry(from, to, run, work, 0)? {
        frames
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        frames.push(frame);
    }
    while let Some(frame) = frames.last_mut() {
        let Some(entry) = frame.entries.next() else {
            frames.pop();
            continue;
        };
        let entry = entry.map_err(|e| contain_trap(run.op, run.pair, &ContainError::from(e)))?;
        let name = entry.file_name();
        if crate::runtime::fs::private_recovery_path(std::path::Path::new(&name)) {
            continue;
        }
        let from = frame.from.child(Arc::clone(&frame.from_dir), name.clone());
        let to = frame.to.child(Arc::clone(&frame.to_dir), name);
        if let Some(child) = copy_entry(&from, &to, run, work, frames.len())? {
            frames
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            frames.push(child);
        }
    }
    Ok(())
}

pub(crate) fn copy_entry(
    from: &LinkPath,
    to: &LinkPath,
    run: &CopyRun<'_>,
    work: &mut CopyWork,
    depth: usize,
) -> wasmtime::Result<Option<CopyFrame>> {
    let CopyRun {
        recursive,
        links,
        op,
        pair,
        quota,
    } = *run;
    work.visit(op, pair)?;
    let meta = from
        .symlink_metadata()
        .map_err(|e| contain_trap(op, pair, &e))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let result = copy_link(from, to);
        // Only nested unrepresentable links are best-effort. Root failures and
        // every move failure retain their existing error behavior.
        if depth == 0 || links == LinkCopies::Required {
            result.map_err(|e| contain_trap(op, pair, &e))?;
        }
        return Ok(None);
    }
    if ft.is_dir() {
        if !recursive {
            wasmtime::bail!("{op} {pair}: source is a directory (use {{ recursive: true }})");
        }
        if depth >= MAX_COPY_DEPTH {
            return Err(crate::runtime::host::range_error(format!(
                "{op} {pair}: exceeds {MAX_COPY_DEPTH} directory levels; copy smaller subtrees"
            )));
        }
        to.create_dir_all()
            .map_err(|e| contain_trap(op, pair, &e))?;
        let from_dir = Arc::new(from.open_dir().map_err(|e| contain_trap(op, pair, &e))?);
        let to_dir = Arc::new(to.open_dir().map_err(|e| contain_trap(op, pair, &e))?);
        let entries = from_dir
            .entries()
            .map_err(|e| contain_trap(op, pair, &ContainError::from(e)))?;
        return Ok(Some(CopyFrame {
            from: from.clone(),
            to: to.clone(),
            from_dir,
            to_dir,
            entries,
        }));
    }
    // A copy may leave earlier files in place on error. Preserve quota claims
    // for whatever landed, and settle known completed bytes on either outcome.
    let mut disk_charge = QuotaCharge::in_place(quota.cloned(), to.copied_onto_file());
    disk_charge
        .reserve(meta.len())
        .map_err(|exceeded| quota_refusal(op, pair, exceeded))?;
    match from.copy_to_counted(to) {
        Ok(bytes) => {
            work.bytes = work.bytes.saturating_add(bytes);
            disk_charge.commit();
        }
        Err(e) => {
            disk_charge.settle_at(to.copied_onto_file().map_or(0, |(_, bytes)| bytes));
            return Err(contain_trap(op, pair, &e));
        }
    }
    Ok(None)
}
