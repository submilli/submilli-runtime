use super::storage::{
    Entries, MAX_PATHS, MAX_REQUESTED_PATHS, Snapshot, memory_limit, validate_branch,
    validate_new_ref_name, validate_path,
};
use crate::runtime::host::invariant_trap;
use gix::bstr::ByteSlice;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use wasmtime::{Result, bail};

pub fn read(snapshot: &Snapshot, op: &str, args: &[Value]) -> Result<Value> {
    match op {
        "status" => status(snapshot),
        "log" => log(snapshot, args.first().unwrap_or(&Value::Null)),
        "diff" => super::diff::read(snapshot, args.first().unwrap_or(&Value::Null)),
        "branches" => branches(snapshot),
        "remotes" => Ok(json!(
            snapshot
                .remotes()?
                .into_iter()
                .map(|(name, url)| json!({"name":name,"url":url}))
                .collect::<Vec<_>>()
        )),
        _ => bail!("git: unknown read operation"),
    }
}

pub fn text_arg(args: &[Value], index: usize) -> Result<&str> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| wasmtime::Error::msg("git: expected a string argument"))
}

pub fn head_entries(snapshot: &Snapshot) -> Result<Entries> {
    let tree = if snapshot.repo.head()?.is_unborn() {
        gix::ObjectId::empty_tree(gix::hash::Kind::Sha1)
    } else {
        super::history::resolve_commit(snapshot, "HEAD")?
            .tree_id()?
            .detach()
    };
    tree_entries(snapshot, tree)
}

/// Every file in the tree `id`, by path, without reading any blob.
pub fn tree_entries(snapshot: &Snapshot, id: gix::ObjectId) -> Result<Entries> {
    let mut files = Entries::new();
    walk_tree(snapshot, id, "", &mut files, &mut 0, 0)?;
    validate_snapshot_files(snapshot, &files)?;
    Ok(files)
}

fn walk_tree(
    snapshot: &Snapshot,
    id: gix::ObjectId,
    prefix: &str,
    files: &mut Entries,
    bytes: &mut usize,
    depth: usize,
) -> Result<()> {
    snapshot.check_cancelled()?;
    if depth > super::storage::MAX_NESTING {
        bail!("git: tree nesting limit exceeded");
    }
    let tree = snapshot.repo.find_tree(id)?;
    snapshot.meter.parse(tree.data.len() as u64);
    // Ancestor tree buffers remain alive during recursion. Charge each whole
    // decoded tree before descending, including entries not yet visited.
    *bytes = bytes.saturating_add(tree.data.len());
    if *bytes > snapshot.max_bytes as usize {
        return Err(memory_limit("tree memory"));
    }
    for entry in tree.iter() {
        snapshot.check_cancelled()?;
        let entry = entry?;
        let path = format!("{prefix}{}", entry.filename().to_str()?);
        validate_path(&path)?;
        *bytes = bytes.saturating_add(path.len().saturating_add(128));
        if *bytes > snapshot.max_bytes as usize {
            return Err(memory_limit("tree path memory"));
        }
        if entry.mode().is_tree() {
            walk_tree(
                snapshot,
                entry.object_id(),
                &format!("{path}/"),
                files,
                bytes,
                depth + 1,
            )?;
        } else {
            let mode: u32 = entry.mode().value() as u32;
            if ![0o100644, 0o100755, 0o120000].contains(&mode) {
                bail!("git: submodules and special tree modes are unsupported");
            }
            if files.len() >= MAX_PATHS {
                return Err(memory_limit("tree resource"));
            }
            snapshot.meter.elements(1);
            if files
                .insert(path, (mode, entry.object_id().to_owned()))
                .is_some()
            {
                bail!("git: duplicate tree path");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// How many times this thread checked a file set, so a test can show that
    /// loading a tree checks it once rather than once per directory.
    pub(super) static FILE_SET_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn validate_snapshot_files<T>(
    snapshot: &Snapshot,
    files: &std::collections::BTreeMap<String, T>,
) -> Result<()> {
    let mut entries = files.len() as u64;
    let mut scanned = 0_u64;
    for path in files.keys() {
        scanned = scanned.saturating_add(path.len() as u64);
        for (offset, _) in path.match_indices('/') {
            entries = entries.saturating_add(1);
            scanned = scanned.saturating_add(offset as u64);
        }
    }
    let log = u64::from(entries.max(2).ilog2()) + 1;
    snapshot.record_algorithm_fuel(
        crate::runtime::fuel::sort_cost(entries)
            .saturating_add(crate::runtime::fuel::SCAN.cost(scanned.saturating_mul(log))),
    )?;
    validate_file_set(files)
}

/// Refuses a set of paths that would collide on a case-folding filesystem, or
/// where one path's file is another's directory.
pub(super) fn validate_file_set<V>(files: &BTreeMap<String, V>) -> Result<()> {
    #[cfg(test)]
    FILE_SET_CHECKS.with(|checks| checks.set(checks.get() + 1));
    let mut folded = BTreeSet::new();
    for path in files.keys() {
        if !folded.insert(path.to_lowercase()) {
            bail!("git: case-colliding tree paths are unsupported");
        }
    }
    let mut directories = std::collections::BTreeMap::new();
    for path in files.keys() {
        for (offset, _) in path.match_indices('/') {
            let directory = &path[..offset];
            let key = directory.to_lowercase();
            if folded.contains(&key) {
                bail!("git: overlapping file and directory paths");
            }
            if directories
                .insert(key, directory)
                .is_some_and(|previous| previous != directory)
            {
                bail!("git: case-colliding tree directories are unsupported");
            }
        }
    }
    Ok(())
}

pub fn index_entries(snapshot: &Snapshot) -> Result<Entries> {
    read_index(snapshot, false)
}

fn read_index(snapshot: &Snapshot, allow_conflicts: bool) -> Result<Entries> {
    let index = snapshot.index()?;
    let mut files = Entries::new();
    let mut bytes = 0;
    for entry in index.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            if allow_conflicts {
                continue;
            }
            bail!("git: resolve index conflicts with native Git before continuing");
        }
        let path = entry.path(&index).to_str()?.to_owned();
        validate_path(&path)?;
        let mode = entry.mode.bits();
        if ![0o100644, 0o100755, 0o120000].contains(&mode) {
            bail!("git: unsupported index mode");
        }
        bytes += path.len() + 128;
        if bytes > snapshot.max_bytes as usize || files.len() >= MAX_PATHS {
            return Err(memory_limit("index resource"));
        }
        files.insert(path, (mode, entry.id));
    }
    validate_snapshot_files(snapshot, &files)?;
    Ok(files)
}

/// Replaces the index with `files`, whose blobs must already be stored.
pub fn write_index(snapshot: &Snapshot, files: &Entries) -> Result<()> {
    let mut state = gix::index::State::new(gix::hash::Kind::Sha1);
    for (path, (mode, id)) in files {
        validate_path(path)?;
        state.dangerously_push_entry(
            Default::default(),
            *id,
            gix::index::entry::Flags::empty(),
            gix::index::entry::Mode::from_bits_truncate(*mode),
            path.as_bytes().as_bstr(),
        );
    }
    state.sort_entries();
    snapshot.write_index(state)
}

fn change(
    before: Option<&(u32, gix::ObjectId)>,
    after: Option<&(u32, gix::ObjectId)>,
) -> &'static str {
    match (before, after) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        (Some(a), Some(b)) if a != b => "modified",
        _ => "unchanged",
    }
}

fn status(snapshot: &Snapshot) -> Result<Value> {
    let raw_index = snapshot.index()?;
    let conflicts: BTreeSet<String> = raw_index
        .entries()
        .iter()
        .filter(|entry| entry.stage() != gix::index::entry::Stage::Unconflicted)
        .map(|entry| entry.path(&raw_index).to_str().map(str::to_owned))
        .collect::<std::result::Result<_, _>>()?;
    let head = head_entries(snapshot)?;
    let index = read_index(snapshot, true)?;
    let mut work = snapshot.worktree()?;
    remove_ignored(snapshot, &mut work, &index)?;
    let paths: BTreeSet<_> = head
        .keys()
        .chain(index.keys())
        .chain(work.keys())
        .chain(conflicts.iter())
        .collect();
    let mut entries = Vec::new();
    for path in paths {
        let staged = if conflicts.contains(path) {
            "conflicted"
        } else {
            change(head.get(path), index.get(path))
        };
        let unstaged = change(index.get(path), work.get(path));
        if staged != "unchanged" || unstaged != "unchanged" {
            entries.push(json!({"path":path,"staged":staged,"unstaged":unstaged,"untracked":!index.contains_key(path) && !conflicts.contains(path) && work.contains_key(path)}));
        }
    }
    let clean = entries.is_empty();
    Ok(without_absent(
        json!({"branch": current_branch(snapshot)?, "entries":entries, "clean":clean}),
    ))
}

/// A `null` member is an absent one: the guest reads the field as `undefined`.
fn without_absent(mut value: Value) -> Value {
    if let Value::Object(fields) = &mut value {
        fields.retain(|_, field| !field.is_null());
    }
    value
}

pub fn current_branch(snapshot: &Snapshot) -> Result<Option<String>> {
    let head = snapshot.repo.head()?;
    let Some(name) = head.referent_name() else {
        return Ok(None);
    };
    snapshot.validate_reference_spelling(name.as_bstr().to_str()?)?;
    let branch = name
        .as_bstr()
        .to_str()?
        .strip_prefix("refs/heads/")
        .ok_or_else(|| wasmtime::Error::msg("git: HEAD must target a local branch"))?;
    if !head.is_unborn() && snapshot.repo.find_reference(name)?.try_id().is_none() {
        bail!(
            "git: symbolic local branch aliases are unsupported; select a direct branch with native Git"
        );
    }
    Ok(Some(branch.to_owned()))
}

fn log(snapshot: &Snapshot, opts: &Value) -> Result<Value> {
    let limit = page_number(opts, "limit", 50)?;
    let offset = page_number(opts, "offset", 0)?;
    if limit == 0 || limit > 1000 {
        bail!("git.log: limit must be 1..1000");
    }
    let mut commits = Vec::new();
    if snapshot.repo.head()?.is_unborn() {
        return Ok(json!({"commits":[]}));
    }
    let head = super::history::resolve_commit(snapshot, "HEAD")?.id;
    let cache = snapshot.history_cache.as_ref().ok_or_else(|| {
        crate::runtime::host::fatal_host_error("git: history cache was not installed")
    })?;
    let ids = cache.page(snapshot, head, offset, limit)?;
    let more = ids.len() > limit as usize;
    commits
        .try_reserve_exact(ids.len().min(limit as usize))
        .map_err(crate::runtime::host::fatal_host_error)?;
    for id in ids.iter().take(limit as usize) {
        snapshot.check_cancelled()?;
        let commit = super::object::commit(snapshot, *id, snapshot.max_bytes)?;
        let decoded = commit.decode()?;
        commits.push(json!({"id":commit.id.to_string(),"message":decoded.message.to_str_lossy(),"authorName":decoded.author()?.name.to_str_lossy(),"authorEmail":decoded.author()?.email.to_str_lossy()}));
    }
    let next = if more {
        Some(
            offset
                .checked_add(limit)
                .ok_or_else(|| wasmtime::Error::msg("git.log: pagination offset overflow"))?,
        )
    } else {
        None
    };
    Ok(without_absent(json!({"commits":commits,"nextOffset":next})))
}

fn page_number(options: &Value, name: &str, default: u64) -> Result<u64> {
    match options.get(name) {
        None => Ok(default),
        Some(value) => value.as_u64().ok_or_else(|| {
            wasmtime::Error::msg(format!("git.log: {name} must be a nonnegative integer"))
        }),
    }
}

pub fn resolve_tree(snapshot: &Snapshot, revision: &str) -> Result<Entries> {
    let commit = super::history::resolve_commit(snapshot, revision)?;
    tree_entries(snapshot, commit.tree_id()?.detach())
}

pub fn show(snapshot: &Snapshot, revision: &str, path: &str) -> Result<Vec<u8>> {
    validate_path(path)?;
    let mut id = super::history::resolve_commit(snapshot, revision)?
        .tree_id()?
        .detach();
    let mut components = path.split('/').peekable();
    let mut bytes = 0_u64;
    let mut depth = 0;
    while let Some(component) = components.next() {
        snapshot.check_cancelled()?;
        if depth > 64 {
            bail!("git: tree nesting limit exceeded");
        }
        let tree = super::object::tree(snapshot, id, snapshot.max_bytes.saturating_sub(bytes) / 4)?;
        bytes = bytes.saturating_add((tree.data.len() as u64).saturating_mul(4));
        if bytes > snapshot.max_bytes {
            bail!("git.show: tree memory limit exceeded");
        }
        let (mode, child) = find_tree_child(snapshot, &tree, component)?
            .ok_or_else(|| wasmtime::Error::msg("git.show: path not found"))?;
        if components.peek().is_some() {
            if !mode.is_tree() {
                bail!("git.show: path not found");
            }
            id = child;
            depth += 1;
            continue;
        }
        if ![0o100644, 0o100755, 0o120000].contains(&(mode.value() as u32)) {
            bail!("git.show: path is not a supported file");
        }
        let blob = super::object::blob(snapshot, child, snapshot.max_bytes.saturating_sub(bytes))?;
        if bytes.saturating_add(blob.data.len() as u64) > snapshot.max_bytes {
            bail!("git.show: blob memory limit exceeded");
        }
        snapshot.record_algorithm_fuel(crate::runtime::fuel::COPY.cost(blob.data.len() as u64))?;
        let mut data = Vec::new();
        data.try_reserve_exact(blob.data.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        data.extend_from_slice(&blob.data);
        return Ok(data);
    }
    bail!("git.show: path not found")
}

/// A blob's contents, refused if larger than the memory available to Git.
fn blob_contents(snapshot: &Snapshot, id: gix::ObjectId) -> Result<Vec<u8>> {
    let size = snapshot.repo.find_header(id)?.size();
    if size > snapshot.max_bytes {
        return Err(memory_limit(&format!("blob size ({id})")));
    }
    snapshot.meter.parse(size);
    Ok(snapshot.repo.find_blob(id)?.detach().data)
}

fn find_tree_child(
    snapshot: &Snapshot,
    tree: &gix::Tree<'_>,
    component: &str,
) -> Result<Option<(gix::objs::tree::EntryMode, gix::ObjectId)>> {
    let mut folded = std::collections::HashSet::new();
    let mut child = None;
    for entry in tree.iter() {
        snapshot.check_cancelled()?;
        let entry = entry?;
        let name = entry.filename().to_str()?;
        validate_path(name)?;
        snapshot.record_algorithm_fuel(
            crate::runtime::fuel::SCAN
                .cost(name.len() as u64 * 2)
                .saturating_add(crate::runtime::fuel::ELEM.cost(1)),
        )?;
        let mut lowercase = String::new();
        lowercase
            .try_reserve(
                name.len()
                    .checked_mul(3)
                    .ok_or_else(|| wasmtime::Error::msg("git.show: filename size overflow"))?,
            )
            .map_err(crate::runtime::host::fatal_host_error)?;
        lowercase.extend(name.chars().flat_map(char::to_lowercase));
        folded
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        if !folded.insert(lowercase) {
            bail!("git: case-colliding tree paths are unsupported");
        }
        if name == component {
            child = Some((entry.mode(), entry.object_id()));
        }
    }
    Ok(child)
}

pub(super) fn append_patch(
    patch: &mut impl std::fmt::Write,
    path: &str,
    before: Option<u32>,
    after: Option<u32>,
    a: &str,
    b: &str,
) -> Result<()> {
    if before
        .zip(after)
        .is_some_and(|(old, new)| old & 0o170000 != new & 0o170000)
    {
        append_patch(patch, path, before, None, a, "")?;
        return append_patch(patch, path, None, after, "", b);
    }
    let old_path = quoted_patch_path('a', path)?;
    let new_path = quoted_patch_path('b', path)?;
    writeln!(patch, "diff --git {old_path} {new_path}")
        .map_err(crate::runtime::host::fatal_host_error)?;
    match (before, after) {
        (None, Some(mode)) => writeln!(patch, "new file mode {mode:06o}"),
        (Some(mode), None) => writeln!(patch, "deleted file mode {mode:06o}"),
        (Some(old), Some(new)) if old != new => {
            writeln!(patch, "old mode {old:06o}\nnew mode {new:06o}")
        }
        _ => Ok(()),
    }
    .map_err(crate::runtime::host::fatal_host_error)?;
    // Empty additions/deletions and mode-only changes need only headers.
    if a == b {
        return Ok(());
    }
    append_patch_hunk(
        patch,
        if before.is_some() {
            &old_path
        } else {
            "/dev/null"
        },
        if after.is_some() {
            &new_path
        } else {
            "/dev/null"
        },
        a,
        b,
    )
}

fn quoted_patch_path(side: char, path: &str) -> Result<String> {
    use std::fmt::Write;
    let quote = path
        .bytes()
        .any(|byte| !matches!(byte, b' '..=b'~') || matches!(byte, b'"' | b'\\'));
    let mut output = String::new();
    output
        .try_reserve_exact(path.len().saturating_mul(4).saturating_add(4))
        .map_err(crate::runtime::host::fatal_host_error)?;
    if quote {
        output.push('"');
    }
    output.push(side);
    output.push('/');
    if quote {
        for byte in path.bytes() {
            match byte {
                7 => output.push_str("\\a"),
                8 => output.push_str("\\b"),
                b'\t' => output.push_str("\\t"),
                b'\n' => output.push_str("\\n"),
                11 => output.push_str("\\v"),
                12 => output.push_str("\\f"),
                b'\r' => output.push_str("\\r"),
                b'"' => output.push_str("\\\""),
                b'\\' => output.push_str("\\\\"),
                b' '..=b'~' => output.push(char::from(byte)),
                byte => write!(output, "\\{byte:03o}")
                    .map_err(crate::runtime::host::fatal_host_error)?,
            }
        }
    } else {
        output.push_str(path);
    }
    if quote {
        output.push('"');
    }
    Ok(output)
}

fn append_patch_hunk(
    patch: &mut impl std::fmt::Write,
    old_path: &str,
    new_path: &str,
    a: &str,
    b: &str,
) -> Result<()> {
    writeln!(
        patch,
        "--- {old_path}\n+++ {new_path}\n@@ -{},{} +{},{} @@",
        usize::from(!a.is_empty()),
        a.split_inclusive('\n').count(),
        usize::from(!b.is_empty()),
        b.split_inclusive('\n').count()
    )
    .map_err(crate::runtime::host::fatal_host_error)?;
    for (sign, text) in [('-', a), ('+', b)] {
        for line in text.split_inclusive('\n') {
            write!(patch, "{sign}{line}").map_err(crate::runtime::host::fatal_host_error)?;
            if !line.ends_with('\n') {
                patch
                    .write_str("\n\\ No newline at end of file\n")
                    .map_err(crate::runtime::host::fatal_host_error)?;
            }
        }
    }
    Ok(())
}

fn branches(snapshot: &Snapshot) -> Result<Value> {
    let current = current_branch(snapshot)?;
    let mut branches = Vec::new();
    let mut bytes = 0;
    snapshot.meter_packed_references()?;
    for reference in snapshot.repo.references()?.local_branches()? {
        snapshot.check_cancelled()?;
        snapshot.meter.syscalls(1);
        snapshot.meter.elements(1);
        let reference = reference.map_err(|error| wasmtime::Error::msg(error.to_string()))?;
        let name = reference.name().shorten().to_str()?.to_owned();
        bytes += name.len() as u64 + 256;
        if bytes > snapshot.max_bytes || branches.len() >= MAX_PATHS {
            return Err(memory_limit("branch listing resource"));
        }
        let id = super::history::follow_reference(snapshot, reference)?;
        branches.push(json!({"name":name,"id":id.to_string(),"current":current.as_deref() == Some(name.as_str())}));
    }
    Ok(json!(branches))
}

pub fn add(snapshot: &Snapshot, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        bail!("git.add: supply at least one path");
    }
    if paths.len() > MAX_REQUESTED_PATHS {
        bail!("git.add: too many paths; stage a containing directory instead");
    }
    let mut work = snapshot.worktree()?;
    let mut index = index_entries(snapshot)?;
    remove_ignored(snapshot, &mut work, &index)?;
    let selected = select_add_paths(snapshot, &index, &work, paths)?;
    for selected in selected {
        snapshot.check_cancelled()?;
        match work.get(&selected) {
            Some(&(mode, id)) => {
                let id = snapshot.store_worktree_blob(&selected, mode, id)?;
                index.insert(selected, (mode, id));
            }
            None => {
                index.remove(&selected);
            }
        }
    }
    validate_snapshot_files(snapshot, &index)?;
    write_index(snapshot, &index)
}

fn select_add_paths(
    snapshot: &Snapshot,
    index: &Entries,
    work: &Entries,
    paths: &[String],
) -> Result<BTreeSet<String>> {
    let file_log = u64::from((index.len().saturating_add(work.len()).max(2)).ilog2()) + 1;
    let request_log = u64::from(paths.len().max(2).ilog2()) + 1;
    let search_units = paths.iter().fold(0_u64, |sum, path| {
        let ancestors = path
            .match_indices('/')
            .fold(0_u64, |sum, (offset, _)| sum.saturating_add(offset as u64));
        sum.saturating_add((path.len() as u64).saturating_mul(2 * file_log + 2 * request_log))
            .saturating_add(ancestors.saturating_mul(request_log))
    });
    snapshot.record_algorithm_fuel(
        crate::runtime::fuel::sort_cost(paths.len() as u64)
            .saturating_add(crate::runtime::fuel::SCAN.cost(search_units)),
    )?;
    let requested: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    for &path in &requested {
        snapshot.check_cancelled()?;
        if path != "." {
            validate_path(path)?;
        }
        let matched = if path == "." {
            !index.is_empty() || !work.is_empty()
        } else {
            let prefix = format!("{path}/");
            matching_paths(index, path, &prefix).next().is_some()
                || matching_paths(work, path, &prefix).next().is_some()
        };
        if !matched {
            bail!("git.add: path does not match a file");
        }
    }
    let mut selected = BTreeSet::new();
    if requested.contains(".") {
        for candidate in index.keys().chain(work.keys()) {
            select_add_path(snapshot, &mut selected, candidate)?;
        }
        return Ok(selected);
    }
    for &path in &requested {
        // Validate every request above, including absent children of a selected parent.
        if path
            .match_indices('/')
            .any(|(offset, _)| requested.contains(&path[..offset]))
        {
            continue;
        }
        let prefix = format!("{path}/");
        for candidate in
            matching_paths(index, path, &prefix).chain(matching_paths(work, path, &prefix))
        {
            select_add_path(snapshot, &mut selected, candidate)?;
        }
    }
    Ok(selected)
}

fn matching_paths<'a>(
    files: &'a Entries,
    path: &str,
    prefix: &'a str,
) -> impl Iterator<Item = &'a String> {
    use std::ops::Bound;
    files
        .get_key_value(path)
        .map(|(path, _)| path)
        .into_iter()
        .chain(
            files
                .range::<str, _>((Bound::Included(prefix), Bound::Unbounded))
                .map(|(path, _)| path)
                .take_while(move |path| path.starts_with(prefix)),
        )
}

fn select_add_path(snapshot: &Snapshot, selected: &mut BTreeSet<String>, path: &str) -> Result<()> {
    snapshot.check_cancelled()?;
    let log = u64::from((selected.len().max(2)).ilog2()) + 1;
    snapshot.record_algorithm_fuel(crate::runtime::fuel::ELEM.cost(log).saturating_add(
        crate::runtime::fuel::SCAN.cost((path.len() as u64).saturating_mul(log)),
    ))?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(path.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    owned.push_str(path);
    selected.insert(owned);
    Ok(())
}

pub fn commit(snapshot: &Snapshot, message: &str, identity: &super::GitConfig) -> Result<String> {
    if message.trim().is_empty() {
        bail!("git.commit: message must not be empty");
    }
    let files = index_entries(snapshot)?;
    if files == head_entries(snapshot)? {
        bail!("git.commit: no staged changes");
    }
    let mut editor = snapshot
        .repo
        .edit_tree(gix::ObjectId::empty_tree(gix::hash::Kind::Sha1))?;
    for (path, (mode, id)) in &files {
        snapshot.check_cancelled()?;
        let kind = match mode {
            0o100755 => gix::objs::tree::EntryKind::BlobExecutable,
            0o120000 => gix::objs::tree::EntryKind::Link,
            _ => gix::objs::tree::EntryKind::Blob,
        };
        editor.upsert(path.as_str(), kind, *id)?;
    }
    let tree = editor.write()?.detach();
    let signature = gix::actor::Signature {
        name: identity.name.clone().into(),
        email: identity.email.clone().into(),
        time: gix::date::Time::now_utc(),
    };
    let parents = if snapshot.repo.head()?.is_unborn() {
        None
    } else {
        Some(super::history::resolve_commit(snapshot, "HEAD")?.id)
    };
    let mut time = Default::default();
    let signature = signature.to_ref(&mut time);
    snapshot.invalidate_reference_cache()?;
    Ok(snapshot
        .repo
        .commit_as(signature, signature, "HEAD", message, tree, parents)?
        .to_string())
}

pub fn create_branch(snapshot: &Snapshot, name: &str, start: &str) -> Result<()> {
    validate_new_ref_name(name)?;
    snapshot.validate_reference_spelling(&format!("refs/heads/{name}"))?;
    let id = super::history::resolve_commit(snapshot, start)?.id;
    snapshot.invalidate_reference_cache()?;
    snapshot.repo.reference(
        format!("refs/heads/{name}"),
        id,
        gix::refs::transaction::PreviousValue::MustNotExist,
        "branch: created",
    )?;
    Ok(())
}

pub fn checkout(snapshot: &Snapshot, branch: &str) -> Result<()> {
    validate_branch(branch)?;
    snapshot.validate_reference_spelling(&format!("refs/heads/{branch}"))?;
    if snapshot
        .repo
        .find_reference(format!("refs/heads/{branch}").as_str())?
        .try_id()
        .is_none()
    {
        bail!(
            "git: symbolic local branch aliases are unsupported; select a direct branch with native Git"
        );
    }
    let next = resolve_tree(snapshot, &format!("refs/heads/{branch}"))?;
    replace_worktree(snapshot, &next)?;
    snapshot.invalidate_reference_cache()?;
    snapshot.write_head(branch)
}

/// Stages the checkout of `next`, which needs a clean worktree: only the
/// files that differ are staged, one blob in memory at a time.
pub fn replace_worktree(snapshot: &Snapshot, next: &Entries) -> Result<()> {
    let previous = index_entries(snapshot)?;
    let work = snapshot.worktree()?;
    if previous != head_entries(snapshot)? || previous != work {
        bail!("git: switching and pulling require a clean working tree, including untracked files");
    }
    validate_snapshot_files(snapshot, next)?;
    let mut pending = snapshot
        .pending_worktree
        .try_borrow_mut()
        .map_err(|_| invariant_trap("git: pending worktree already borrowed"))?;
    let mut change = super::stage::WorktreeChange::default();
    // Every file that goes or changes is removed before any is placed, so a
    // file can take a directory's place, or a name differing only in case.
    for (path, entry) in &previous {
        if next.get(path) != Some(entry) {
            change.remove.push(path.clone());
        }
    }
    for (path, entry) in next {
        snapshot.check_cancelled()?;
        if previous.get(path) == Some(entry) {
            continue;
        }
        let (mode, id) = *entry;
        snapshot.stage_worktree_file(path, mode, &blob_contents(snapshot, id)?)?;
        change.place.push(path.clone());
    }
    write_index(snapshot, next)?;
    *pending = Some(change);
    Ok(())
}

pub fn set_remote(snapshot: &mut Snapshot, name: &str, url: &str, add: bool) -> Result<()> {
    if add {
        validate_new_ref_name(name)?;
    } else {
        validate_branch(name)?;
    }
    let url = super::transport::canonical_url(url)?;
    let remotes = snapshot.remotes()?;
    if add == remotes.contains_key(name) {
        bail!(
            "git: remote already exists or was not found; use addRemote or setRemoteUrl appropriately"
        );
    }
    let mut config = gix::config::File::from_bytes_no_includes(
        &snapshot.config,
        gix::config::file::Metadata::default(),
        Default::default(),
    )?;
    // URLs are multivalued in native Git: replacing only the last one leaves
    // native fetch using the old first URL. Preserve every unrelated setting.
    if !add {
        config
            .raw_values_mut_by("remote", Some(name.as_bytes().as_bstr()), "url")?
            .delete_all();
    }
    config.set_raw_value_by("remote", Some(name.as_bytes().as_bstr()), "url", url)?;
    if add {
        config.set_raw_value_by(
            "remote",
            Some(name.as_bytes().as_bstr()),
            "fetch",
            format!("+refs/heads/*:refs/remotes/{name}/*"),
        )?;
    }
    snapshot.set_config(config.to_bstring().into())
}

fn remove_ignored(snapshot: &Snapshot, work: &mut Entries, index: &Entries) -> Result<()> {
    snapshot.check_cancelled()?;
    let mut search = gix::ignore::Search::default();
    let mut budget = IgnoreBudget::default();
    if let Some(bytes) = super::storage::read_bounded(
        &snapshot.dir,
        std::path::Path::new(".git/info/exclude"),
        snapshot.max_bytes,
    )? {
        budget.add(snapshot, &mut search, &bytes, ".gitignore")?;
    }
    for (path, (mode, _)) in work.iter() {
        snapshot.check_cancelled()?;
        if *mode != 0o120000 && (path == ".gitignore" || path.ends_with("/.gitignore")) {
            let bytes = snapshot.worktree_contents(path, snapshot.max_bytes)?;
            budget.add(snapshot, &mut search, &bytes, path)?;
        }
    }
    let mut remaining_work = snapshot.max_bytes.saturating_mul(16).min(50_000_000);
    let mut ignored = Vec::new();
    for path in work.keys() {
        snapshot.check_cancelled()?;
        if index.contains_key(path) {
            continue;
        }
        for (offset, _) in path
            .match_indices('/')
            .chain(std::iter::once((path.len(), "/")))
        {
            snapshot.check_cancelled()?;
            let cost = budget.pattern_bytes + budget.patterns * (offset as u64 + 1);
            remaining_work = remaining_work
                .checked_sub(cost)
                .ok_or_else(|| memory_limit("ignore matching resource"))?;
            snapshot.meter.scan(cost);
            if search
                .pattern_matching_relative_path(
                    path.as_bytes()[..offset].as_bstr(),
                    Some(offset < path.len()),
                    gix::glob::pattern::Case::Sensitive,
                )
                .is_some_and(|matched| !matched.pattern.is_negative())
            {
                ignored.push(path.clone());
                break;
            }
        }
    }
    for path in ignored {
        work.remove(&path);
    }
    Ok(())
}

#[derive(Default)]
struct IgnoreBudget {
    patterns: u64,
    pattern_bytes: u64,
}

impl IgnoreBudget {
    fn add(
        &mut self,
        snapshot: &Snapshot,
        search: &mut gix::ignore::Search,
        bytes: &[u8],
        source: &str,
    ) -> Result<()> {
        // Count before parsing: a two-byte line allocates an entire pattern
        // record, and all parsed lists stay alive throughout matching.
        for line in bytes.split(|byte| *byte == b'\n') {
            snapshot.check_cancelled()?;
            if line.is_empty() || line.starts_with(b"#") {
                continue;
            }
            self.patterns += 1;
            self.pattern_bytes += line.len() as u64;
            if line.len() > 4096
                || self.patterns > 10_000
                || self.pattern_bytes + self.patterns * 128 > snapshot.max_bytes
            {
                return Err(memory_limit("ignore pattern resource"));
            }
        }
        search.add_patterns_buffer(
            bytes,
            source,
            Some(std::path::Path::new("")),
            Default::default(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_accepts_large_offsets_and_returns_followable_pages() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let mut snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            16_384,
        )
        .unwrap();
        snapshot.history_cache = Some(std::sync::Arc::new(
            super::super::log_cache::Cache::new(&crate::runtime::limits::TenantLimits::new(
                8 * 1024 * 1024,
            ))
            .unwrap(),
        ));
        let exhausted = json!({"commits":[]});
        assert_eq!(
            log(&snapshot, &json!({"offset":10_001})).unwrap(),
            exhausted
        );
        let identity = super::super::GitConfig {
            name: "Test".into(),
            email: "test@example.com".into(),
            username: None,
        };
        let mut ids = Vec::new();
        for contents in ["first", "second"] {
            snapshot.dir.write("file", contents).unwrap();
            add(&snapshot, &["file".into()]).unwrap();
            ids.push(commit(&snapshot, contents, &identity).unwrap());
        }
        let first = log(&snapshot, &json!({"limit":1})).unwrap();
        assert_eq!(first["commits"][0]["id"], ids[1]);
        assert_eq!(first["nextOffset"], 1);
        let second = log(&snapshot, &json!({"limit":1,"offset":first["nextOffset"]})).unwrap();
        assert_eq!(second["commits"][0]["id"], ids[0]);
        assert!(second.get("nextOffset").is_none());
        for offset in [10_001, u64::MAX] {
            assert_eq!(
                log(&snapshot, &json!({"offset":offset})).unwrap(),
                exhausted
            );
        }
    }

    #[test]
    fn ignore_patterns_bound_decoded_memory_matching_work_and_cancellation() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            4096,
        )
        .unwrap();
        snapshot
            .dir
            .write(".gitignore", b"a\n".repeat(2048))
            .unwrap();
        let mut work = snapshot.worktree().unwrap();
        assert!(
            remove_ignored(&snapshot, &mut work, &Entries::new())
                .unwrap_err()
                .to_string()
                .contains("ignore pattern resource")
        );
        snapshot.dir.write(".gitignore", b"a\n".repeat(20)).unwrap();
        let mut work = Entries::from([(
            ".gitignore".into(),
            (0o100644, gix::ObjectId::null(gix::hash::Kind::Sha1)),
        )]);
        for number in 0..1000 {
            work.insert(
                format!("file{number:04}"),
                (0o100644, gix::ObjectId::null(gix::hash::Kind::Sha1)),
            );
        }
        assert!(
            remove_ignored(&snapshot, &mut work, &Entries::new())
                .unwrap_err()
                .to_string()
                .contains("ignore matching resource")
        );
        snapshot
            .cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(
            remove_ignored(&snapshot, &mut work, &Entries::new())
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
    }

    #[test]
    fn staging_bounds_requests_and_unions_duplicate_selections() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            4096,
        )
        .unwrap();
        snapshot.dir.write("file", "contents").unwrap();
        assert!(
            add(&snapshot, &vec![".".to_owned(); MAX_REQUESTED_PATHS + 1])
                .unwrap_err()
                .to_string()
                .contains("too many paths")
        );
        assert!(add(&snapshot, &[".".into(), "missing".into()]).is_err());
        assert!(index_entries(&snapshot).unwrap().is_empty());
        add(&snapshot, &vec![".".to_owned(); MAX_REQUESTED_PATHS]).unwrap();
        let id = index_entries(&snapshot).unwrap()["file"].1;
        assert_eq!(snapshot.repo.find_blob(id).unwrap().data, b"contents");
        snapshot
            .cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(
            add(&snapshot, &[".".into()])
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
    }

    #[test]
    fn nested_trees_charge_unvisited_entries_before_descending() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            4096,
        )
        .unwrap();
        let blob = snapshot.repo.write_blob([]).unwrap().detach();
        let mut child = None;
        for _ in 0..30 {
            let mut entries = Vec::new();
            if let Some(oid) = child {
                entries.push(gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Tree.into(),
                    filename: "a".into(),
                    oid,
                });
            }
            for number in 0..70 {
                entries.push(gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: format!("z{number:03}").into(),
                    oid: blob,
                });
            }
            child = Some(
                snapshot
                    .repo
                    .write_object(&gix::objs::Tree { entries })
                    .unwrap()
                    .detach(),
            );
        }
        let error = tree_entries(&snapshot, child.unwrap()).unwrap_err();
        assert!(error.to_string().contains("tree memory limit exceeded"));
    }

    #[test]
    fn loading_a_tree_checks_its_file_set_once() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            1 << 20,
        )
        .unwrap();
        let blob = snapshot.repo.write_blob(b"x").unwrap().detach();
        let mut editor = snapshot
            .repo
            .edit_tree(gix::ObjectId::empty_tree(gix::hash::Kind::Sha1))
            .unwrap();
        for directory in 0..50 {
            for file in 0..4 {
                editor
                    .upsert(
                        format!("d{directory}/e/f{file}"),
                        gix::objs::tree::EntryKind::Blob,
                        blob,
                    )
                    .unwrap();
            }
        }
        let tree = editor.write().unwrap().detach();
        FILE_SET_CHECKS.with(|checks| checks.set(0));
        let entries = tree_entries(&snapshot, tree).unwrap();
        assert_eq!(entries.len(), 200);
        assert_eq!(FILE_SET_CHECKS.with(std::cell::Cell::get), 1);
    }

    #[test]
    fn status_and_add_hash_files_without_reading_them_whole() {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        // Far less memory than the file holds.
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            64 * 1024,
        )
        .unwrap();
        let large = vec![b'x'; 1 << 20];
        snapshot.dir.write("large", &large).unwrap();
        assert_eq!(status(&snapshot).unwrap()["entries"][0]["path"], "large");
        add(&snapshot, &["large".into()]).unwrap();
        let (_, id) = index_entries(&snapshot).unwrap()["large"];
        assert_eq!(
            id,
            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, &large).unwrap()
        );
        assert!(snapshot.repo.has_object(id));
    }
}
