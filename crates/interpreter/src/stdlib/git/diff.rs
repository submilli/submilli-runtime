//! Compare Git object identities before inflating changed file contents.
use super::storage::{Files, MAX_PATHS, Snapshot, validate_path};
use crate::runtime::fuel;
use gix::bstr::ByteSlice;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use wasmtime::{Result, bail};

type Objects = BTreeMap<String, (u32, gix::ObjectId)>;

struct Source {
    objects: Objects,
    worktree: Option<Files>,
    bytes: usize,
    loaded_bytes: Cell<usize>,
}

pub(super) fn read(snapshot: &Snapshot, options: &Value) -> Result<Value> {
    let mode = options
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("working");
    let (before, after) = sources(snapshot, options, mode)?;
    let count = before.objects.len().saturating_add(after.objects.len()) as u64;
    let path_bytes = before
        .objects
        .keys()
        .chain(after.objects.keys())
        .fold(0_u64, |sum, path| sum.saturating_add(path.len() as u64));
    let log = u64::from(count.max(2).ilog2()) + 1;
    snapshot.record_algorithm_fuel(
        fuel::sort_cost(count).saturating_add(fuel::SCAN.cost(path_bytes.saturating_mul(log))),
    )?;
    let paths: BTreeSet<_> = before.objects.keys().chain(after.objects.keys()).collect();
    let mut patch = String::new();
    let mut binary = Vec::new();
    for path in paths {
        snapshot.check_cancelled()?;
        if before.objects.get(path) == after.objects.get(path)
            || (mode == "working" && !before.objects.contains_key(path))
        {
            continue;
        }
        let a = before.load(snapshot, path)?;
        let b = after.load(snapshot, path)?;
        snapshot.record_algorithm_fuel(
            fuel::SCAN.cost(
                (a.len() as u64)
                    .saturating_add(b.len() as u64)
                    .saturating_mul(6),
            ),
        )?;
        let (Ok(old), Ok(new)) = (std::str::from_utf8(&a), std::str::from_utf8(&b)) else {
            binary
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            binary.push(path);
            continue;
        };
        if a.contains(&0) || b.contains(&0) {
            binary
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            binary.push(path);
            continue;
        }
        append(
            snapshot,
            &mut patch,
            path,
            (
                before.objects.get(path).map(|v| v.0),
                after.objects.get(path).map(|v| v.0),
            ),
            old,
            new,
        )?;
    }
    Ok(json!({"patch":patch,"binaryPaths":binary}))
}

fn sources(snapshot: &Snapshot, options: &Value, mode: &str) -> Result<(Source, Source)> {
    match mode {
        "working" => Ok((index(snapshot)?, working(snapshot)?)),
        "staged" => {
            let head = if snapshot.repo.head()?.is_unborn() {
                Source::empty()
            } else {
                tree(snapshot, "HEAD")?
            };
            Ok((head, index(snapshot)?))
        }
        "refs" => Ok((
            tree(snapshot, revision(options, "from")?)?,
            tree(snapshot, revision(options, "to")?)?,
        )),
        _ => bail!("git.diff: mode must be working, staged, or refs"),
    }
}

fn revision<'a>(options: &'a Value, name: &str) -> Result<&'a str> {
    options
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| wasmtime::Error::msg(format!("git.diff: {name} is required")))
}

impl Source {
    fn empty() -> Self {
        Self {
            objects: Objects::new(),
            worktree: None,
            bytes: 0,
            loaded_bytes: Cell::new(0),
        }
    }

    fn insert(&mut self, path: String, mode: u32, id: gix::ObjectId) -> Result<()> {
        validate_path(&path)?;
        if ![0o100644, 0o100755, 0o120000].contains(&mode) {
            bail!("git: submodules and special file modes are unsupported");
        }
        if self.objects.len() >= MAX_PATHS {
            bail!("git: file path limit exceeded");
        }
        if self.objects.insert(path, (mode, id)).is_some() {
            bail!("git: duplicate file path");
        }
        Ok(())
    }

    fn admit(&mut self, snapshot: &Snapshot, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > snapshot.max_bytes as usize {
            bail!("git.diff: memory limit exceeded");
        }
        Ok(())
    }

    fn load<'a>(&'a self, snapshot: &Snapshot, path: &str) -> Result<Cow<'a, [u8]>> {
        let Some((_, id)) = self.objects.get(path) else {
            return Ok(Cow::Borrowed(&[]));
        };
        if let Some(work) = &self.worktree {
            return work
                .get(path)
                .map(|value| Cow::Borrowed(value.1.as_slice()))
                .ok_or_else(|| {
                    crate::runtime::host::fatal_host_error("git.diff: missing worktree data")
                });
        }
        let remaining = snapshot
            .max_bytes
            .saturating_sub(self.bytes as u64)
            .saturating_sub(self.loaded_bytes.get() as u64);
        let blob = super::object::blob(snapshot, *id, remaining)?;
        let loaded = self.loaded_bytes.get().saturating_add(blob.data.len());
        let bytes = self.bytes.saturating_add(loaded);
        if bytes > snapshot.max_bytes as usize {
            bail!("git.diff: memory limit exceeded");
        }
        self.loaded_bytes.set(loaded);
        snapshot.record_algorithm_fuel(fuel::COPY.cost(blob.data.len() as u64))?;
        let mut data = Vec::new();
        data.try_reserve_exact(blob.data.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        data.extend_from_slice(&blob.data);
        Ok(Cow::Owned(data))
    }
}

fn tree(snapshot: &Snapshot, revision: &str) -> Result<Source> {
    let id = super::history::resolve_commit(snapshot, revision)?
        .tree_id()?
        .detach();
    let mut source = Source::empty();
    let mut pending = Vec::new();
    pending
        .try_reserve(1)
        .map_err(crate::runtime::host::fatal_host_error)?;
    pending.push((id, String::new(), 0_usize));
    while let Some((id, prefix, depth)) = pending.pop() {
        snapshot.check_cancelled()?;
        if depth > 64 {
            bail!("git: tree nesting limit exceeded");
        }
        let tree = super::object::tree(
            snapshot,
            id,
            snapshot.max_bytes.saturating_sub(source.bytes as u64),
        )?;
        source.admit(snapshot, tree.data.len())?;
        for entry in tree.iter() {
            snapshot.check_cancelled()?;
            let entry = entry?;
            let name = entry.filename().to_str()?;
            let len = prefix.len().saturating_add(name.len());
            source.admit(snapshot, len.saturating_add(128))?;
            snapshot.record_algorithm_fuel(fuel::SCAN.cost(len as u64))?;
            let mut path = String::new();
            path.try_reserve_exact(len.saturating_add(1))
                .map_err(crate::runtime::host::fatal_host_error)?;
            path.push_str(&prefix);
            path.push_str(name);
            validate_path(&path)?;
            if entry.mode().is_tree() {
                path.push('/');
                pending
                    .try_reserve(1)
                    .map_err(crate::runtime::host::fatal_host_error)?;
                pending.push((entry.object_id(), path, depth + 1));
            } else {
                source.insert(path, entry.mode().value() as u32, entry.object_id())?;
            }
        }
    }
    super::operations::validate_snapshot_files(snapshot, &source.objects)?;
    Ok(source)
}

fn index(snapshot: &Snapshot) -> Result<Source> {
    let index = snapshot.repo.index_or_empty()?;
    let mut source = Source::empty();
    for entry in index.entries() {
        snapshot.check_cancelled()?;
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            bail!("git: resolve index conflicts with native Git before continuing");
        }
        let path = entry.path(&index).to_str()?;
        snapshot.record_algorithm_fuel(fuel::SCAN.cost(path.len() as u64))?;
        source.admit(snapshot, path.len().saturating_add(128))?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(path.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        owned.push_str(path);
        source.insert(owned, entry.mode.bits(), entry.id)?;
    }
    super::operations::validate_snapshot_files(snapshot, &source.objects)?;
    Ok(source)
}

fn working(snapshot: &Snapshot) -> Result<Source> {
    let worktree = snapshot.worktree()?;
    let mut source = Source::empty();
    for (path, (mode, data)) in &worktree {
        snapshot.check_cancelled()?;
        snapshot.record_algorithm_fuel(fuel::HASH.cost(data.len() as u64))?;
        let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, data)?;
        source.admit(snapshot, path.len().saturating_add(128))?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(path.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        owned.push_str(path);
        source.insert(owned, *mode, id)?;
        source.admit(
            snapshot,
            data.len().saturating_add(path.len()).saturating_add(128),
        )?;
    }
    source.worktree = Some(worktree);
    Ok(source)
}

fn append(
    snapshot: &Snapshot,
    patch: &mut String,
    path: &str,
    modes: (Option<u32>, Option<u32>),
    old: &str,
    new: &str,
) -> Result<()> {
    // Count the complete patch before allocating it, including escaped paths
    // and the two headers required for file/symlink type changes.
    let mut size = PatchSize(0);
    super::operations::append_patch(&mut size, path, modes.0, modes.1, old, new)?;
    let remaining = (snapshot.max_bytes as usize).saturating_sub(patch.len());
    if size.0 > remaining {
        bail!("git.diff: output limit exceeded");
    }
    let mut output = String::new();
    output
        .try_reserve_exact(size.0)
        .map_err(crate::runtime::host::fatal_host_error)?;
    super::operations::append_patch(&mut output, path, modes.0, modes.1, old, new)?;
    snapshot.record_algorithm_fuel(fuel::COPY.cost(output.len() as u64))?;
    patch
        .try_reserve_exact(output.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    patch.push_str(&output);
    Ok(())
}

struct PatchSize(usize);
impl std::fmt::Write for PatchSize {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 = self.0.saturating_add(text.len());
        Ok(())
    }
}
