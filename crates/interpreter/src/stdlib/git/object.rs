//! Read object headers to admit decoding before inflating object bodies.
use super::storage::Snapshot;
use crate::runtime::fuel;
use wasmtime::{Result, bail};

pub(super) fn tree(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    remaining: u64,
) -> Result<gix::Tree<'_>> {
    let size = admit(snapshot, id, gix::objs::Kind::Tree, remaining)?;
    let tree = snapshot.repo.find_tree(id)?;
    if tree.data.len() as u64 != size {
        bail!("git: tree size disagrees with its header");
    }
    Ok(tree)
}

pub(super) fn blob(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    remaining: u64,
) -> Result<gix::Blob<'_>> {
    let size = admit(snapshot, id, gix::objs::Kind::Blob, remaining)?;
    let blob = snapshot.repo.find_blob(id)?;
    if blob.data.len() as u64 != size {
        bail!("git: blob size disagrees with its header");
    }
    Ok(blob)
}

pub(super) fn commit(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    remaining: u64,
) -> Result<gix::Commit<'_>> {
    let size = admit(snapshot, id, gix::objs::Kind::Commit, remaining)?;
    let commit = snapshot.repo.find_commit(id)?;
    if commit.data.len() as u64 != size {
        bail!("git: commit size disagrees with its header");
    }
    Ok(commit)
}

pub(super) fn read(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    remaining: u64,
) -> Result<gix::Object<'_>> {
    snapshot.check_cancelled()?;
    snapshot.record_algorithm_fuel(fuel::SYSCALL.cost(1))?;
    let header = snapshot.repo.find_header(id)?;
    let size = header.size();
    let kind = header.kind();
    if size > remaining {
        bail!("git: decoded object memory limit exceeded");
    }
    snapshot.record_algorithm_fuel(fuel::PARSE.cost(size))?;
    let object = snapshot.repo.find_object(id)?;
    if object.data.len() as u64 != size || object.kind != kind {
        return Err(crate::runtime::host::fatal_host_error(
            "git: object disagrees with its header",
        ));
    }
    Ok(object)
}

fn admit(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    kind: gix::objs::Kind,
    remaining: u64,
) -> Result<u64> {
    snapshot.check_cancelled()?;
    snapshot.record_algorithm_fuel(fuel::SYSCALL.cost(1))?;
    let header = snapshot.repo.find_header(id)?;
    if header.kind() != kind {
        bail!("git: unexpected object kind");
    }
    let size = header.size();
    if size > remaining {
        match kind {
            gix::objs::Kind::Tree => bail!("git: tree memory limit exceeded"),
            gix::objs::Kind::Blob => bail!("git: blob memory limit exceeded"),
            _ => bail!("git: decoded object memory limit exceeded"),
        }
    }
    snapshot.record_algorithm_fuel(fuel::PARSE.cost(size))?;
    Ok(size)
}
