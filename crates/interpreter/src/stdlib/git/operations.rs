use super::storage::{
    Entries, MAX_PATHS, Snapshot, validate_branch, validate_new_ref_name, validate_path,
};
use gix::bstr::ByteSlice;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use wasmtime::{Result, bail};

pub fn read(snapshot: &Snapshot, op: &str, args: &[Value]) -> Result<Value> {
    match op {
        "status" => status(snapshot),
        "log" => log(snapshot, args.first().unwrap_or(&Value::Null)),
        "diff" => diff(snapshot, args.first().unwrap_or(&Value::Null)),
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
    validate_file_set(&files)?;
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
    if depth > 64 {
        bail!("git: tree nesting limit exceeded");
    }
    let tree = snapshot.repo.find_tree(id)?;
    snapshot.meter.parse(tree.data.len() as u64);
    // Ancestor tree buffers remain alive during recursion. Charge each whole
    // decoded tree before descending, including entries not yet visited.
    *bytes += tree.data.len();
    if *bytes > snapshot.max_bytes as usize {
        bail!("git: tree memory limit exceeded");
    }
    for entry in tree.iter() {
        snapshot.check_cancelled()?;
        let entry = entry?;
        let path = format!("{prefix}{}", entry.filename().to_str()?);
        validate_path(&path)?;
        *bytes += path.len() + 128;
        if *bytes > snapshot.max_bytes as usize {
            bail!("git: tree path memory limit exceeded");
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
                bail!("git: tree resource limit exceeded");
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
            bail!("git: index resource limit exceeded");
        }
        files.insert(path, (mode, entry.id));
    }
    validate_file_set(&files)?;
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
    Ok(json!({"branch": current_branch(snapshot)?, "entries":entries, "clean":entries.is_empty()}))
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
        return Ok(json!({"commits":[],"nextOffset":null}));
    }
    let head = super::history::resolve_commit(snapshot, "HEAD")?.id;
    let mut history = super::history::Ancestors::new(snapshot, head)?;
    let mut skipped = 0;
    while let Some(commit) = history.next()? {
        if skipped < offset {
            skipped += 1;
            continue;
        }
        if commits.len() == limit as usize {
            let next_offset = offset
                .checked_add(limit)
                .ok_or_else(|| wasmtime::Error::msg("git.log: pagination offset overflow"))?;
            return Ok(json!({"commits":commits,"nextOffset":next_offset}));
        }
        let decoded = commit.decode()?;
        commits.push(json!({"id":commit.id.to_string(),"message":decoded.message.to_str_lossy(),"authorName":decoded.author()?.name.to_str_lossy(),"authorEmail":decoded.author()?.email.to_str_lossy()}));
    }
    Ok(json!({"commits":commits,"nextOffset":null}))
}

fn page_number(options: &Value, name: &str, default: u64) -> Result<u64> {
    match options.get(name).filter(|value| !value.is_null()) {
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
    let mut tree = super::history::resolve_commit(snapshot, revision)?.tree()?;
    let entry = tree
        .peel_to_entry_by_path(path)?
        .filter(|entry| entry.mode().is_blob_or_symlink())
        .ok_or_else(|| wasmtime::Error::msg("git.show: path not found"))?;
    blob_contents(snapshot, entry.object_id())
}

/// A blob's contents, refused if larger than the memory available to Git.
fn blob_contents(snapshot: &Snapshot, id: gix::ObjectId) -> Result<Vec<u8>> {
    let size = snapshot.repo.find_header(id)?.size();
    if size > snapshot.max_bytes {
        bail!("git: blob {id} exceeds the memory available to Git");
    }
    snapshot.meter.parse(size);
    Ok(snapshot.repo.find_blob(id)?.detach().data)
}

/// Where the contents of a side of a diff come from.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Objects,
    Worktree,
}

fn contents(
    snapshot: &Snapshot,
    source: Source,
    path: &str,
    entry: Option<&(u32, gix::ObjectId)>,
) -> Result<Vec<u8>> {
    match (entry, source) {
        (None, _) => Ok(Vec::new()),
        (Some((_, id)), Source::Objects) => blob_contents(snapshot, *id),
        (Some(_), Source::Worktree) => snapshot.worktree_contents(path, snapshot.max_bytes),
    }
}

fn diff(snapshot: &Snapshot, options: &Value) -> Result<Value> {
    let mode = options
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("working");
    let after_source = if mode == "working" {
        Source::Worktree
    } else {
        Source::Objects
    };
    let (before, after) = match mode {
        "working" => (index_entries(snapshot)?, snapshot.worktree()?),
        "staged" => (head_entries(snapshot)?, index_entries(snapshot)?),
        "refs" => (
            resolve_tree(
                snapshot,
                options["from"]
                    .as_str()
                    .ok_or_else(|| wasmtime::Error::msg("git.diff: from is required"))?,
            )?,
            resolve_tree(
                snapshot,
                options["to"]
                    .as_str()
                    .ok_or_else(|| wasmtime::Error::msg("git.diff: to is required"))?,
            )?,
        ),
        _ => bail!("git.diff: mode must be working, staged, or refs"),
    };
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut patch = String::new();
    let mut binary = Vec::new();
    for path in paths {
        if before.get(path) == after.get(path) {
            continue;
        }
        if mode == "working" && !before.contains_key(path) {
            continue;
        }
        // One pair of contents in memory at a time.
        let a = contents(snapshot, Source::Objects, path, before.get(path))?;
        let b = contents(snapshot, after_source, path, after.get(path))?;
        let (Ok(a), Ok(b)) = (std::str::from_utf8(&a), std::str::from_utf8(&b)) else {
            binary.push(path);
            continue;
        };
        if a.contains('\0') || b.contains('\0') {
            binary.push(path);
            continue;
        }
        append_patch(
            &mut patch,
            path,
            before.get(path).map(|v| v.0),
            after.get(path).map(|v| v.0),
            a,
            b,
        );
        if patch.len() > snapshot.max_bytes as usize {
            bail!("git.diff: output limit exceeded");
        }
    }
    Ok(json!({"patch":patch,"binaryPaths":binary}))
}

fn append_patch(
    patch: &mut String,
    path: &str,
    before: Option<u32>,
    after: Option<u32>,
    a: &str,
    b: &str,
) {
    if before
        .zip(after)
        .is_some_and(|(old, new)| old & 0o170000 != new & 0o170000)
    {
        append_patch(patch, path, before, None, a, "");
        append_patch(patch, path, None, after, "", b);
        return;
    }
    let old_path = format!("a/{path}");
    let new_path = format!("b/{path}");
    let old_path = gix::quote::ansi_c::quote(old_path.as_bytes().as_bstr());
    let new_path = gix::quote::ansi_c::quote(new_path.as_bytes().as_bstr());
    patch.push_str(&format!("diff --git {old_path} {new_path}\n"));
    match (before, after) {
        (None, Some(mode)) => patch.push_str(&format!("new file mode {mode:06o}\n")),
        (Some(mode), None) => patch.push_str(&format!("deleted file mode {mode:06o}\n")),
        (Some(old), Some(new)) if old != new => {
            patch.push_str(&format!("old mode {old:06o}\nnew mode {new:06o}\n"));
        }
        _ => {}
    }
    // Empty additions/deletions and mode-only changes are completely described by headers.
    if a == b {
        return;
    }
    append_patch_hunk(
        patch,
        if before.is_some() {
            old_path.as_ref()
        } else {
            b"/dev/null".as_bstr()
        },
        if after.is_some() {
            new_path.as_ref()
        } else {
            b"/dev/null".as_bstr()
        },
        a,
        b,
    );
}

fn append_patch_hunk(
    patch: &mut String,
    old_path: &gix::bstr::BStr,
    new_path: &gix::bstr::BStr,
    a: &str,
    b: &str,
) {
    patch.push_str(&format!(
        "--- {old_path}\n+++ {new_path}\n@@ -{},{} +{},{} @@\n",
        usize::from(!a.is_empty()),
        a.split_inclusive('\n').count(),
        usize::from(!b.is_empty()),
        b.split_inclusive('\n').count()
    ));
    for (sign, text) in [('-', a), ('+', b)] {
        for line in text.split_inclusive('\n') {
            patch.push(sign);
            patch.push_str(line);
            if !line.ends_with('\n') {
                patch.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
}

fn branches(snapshot: &Snapshot) -> Result<Value> {
    let current = current_branch(snapshot)?;
    let mut branches = Vec::new();
    let mut bytes = 0;
    for reference in snapshot.repo.references()?.local_branches()? {
        snapshot.check_cancelled()?;
        let reference = reference.map_err(|error| wasmtime::Error::msg(error.to_string()))?;
        let name = reference.name().shorten().to_str()?.to_owned();
        bytes += name.len() as u64 + 256;
        if bytes > snapshot.max_bytes || branches.len() >= MAX_PATHS {
            bail!("git: branch listing resource limit exceeded");
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
    if paths.len() > MAX_PATHS {
        bail!("git.add: too many paths; stage a containing directory instead");
    }
    let mut work = snapshot.worktree()?;
    let mut index = index_entries(snapshot)?;
    remove_ignored(snapshot, &mut work, &index)?;
    let mut selected = BTreeSet::new();
    for path in paths.iter().collect::<BTreeSet<_>>() {
        snapshot.check_cancelled()?;
        let mut matched = false;
        if path == "." {
            matched = !(index.is_empty() && work.is_empty());
            selected.extend(index.keys().chain(work.keys()).cloned());
        } else {
            validate_path(path)?;
            let prefix = format!("{path}/");
            for set in [&index, &work] {
                if set.contains_key(path.as_str()) {
                    matched = true;
                    selected.insert(path.clone());
                }
                // The paths below `path/` sort together, from `path/` on.
                for candidate in set
                    .range::<str, _>((
                        std::ops::Bound::Included(prefix.as_str()),
                        std::ops::Bound::Unbounded,
                    ))
                    .map(|(candidate, _)| candidate)
                    .take_while(|candidate| candidate.starts_with(&prefix))
                {
                    snapshot.check_cancelled()?;
                    matched = true;
                    selected.insert(candidate.clone());
                }
            }
        }
        if !matched {
            bail!("git.add: path does not match a file");
        }
    }
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
    validate_file_set(&index)?;
    write_index(snapshot, &index)
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
    Ok(snapshot
        .repo
        .commit_as(signature, signature, "HEAD", message, tree, parents)?
        .to_string())
}

pub fn create_branch(snapshot: &Snapshot, name: &str, start: &str) -> Result<()> {
    validate_new_ref_name(name)?;
    snapshot.validate_reference_spelling(&format!("refs/heads/{name}"))?;
    let id = super::history::resolve_commit(snapshot, start)?.id;
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
    validate_file_set(next)?;
    let mut change = super::stage::WorktreeChange::default();
    for path in previous.keys() {
        if !next.contains_key(path) {
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
    *snapshot.pending_worktree.borrow_mut() = Some(change);
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
        &snapshot.original_config,
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
    match snapshot.dir.open(".git/info/exclude") {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(snapshot.max_bytes.saturating_add(1))
                .read_to_end(&mut bytes)?;
            budget.add(snapshot, &mut search, &bytes, ".gitignore")?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
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
            remaining_work = remaining_work.checked_sub(cost).ok_or_else(|| {
                wasmtime::Error::msg("git: ignore matching resource limit exceeded")
            })?;
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
                bail!("git: ignore pattern resource limit exceeded");
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
        let snapshot = Snapshot::init(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            16_384,
        )
        .unwrap();
        let exhausted = json!({"commits":[],"nextOffset":null});
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
        assert_eq!(second["nextOffset"], Value::Null);
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
        let snapshot = Snapshot::init(
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
        let snapshot = Snapshot::init(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            4096,
        )
        .unwrap();
        snapshot.dir.write("file", "contents").unwrap();
        assert!(
            add(&snapshot, &vec![".".to_owned(); MAX_PATHS + 1])
                .unwrap_err()
                .to_string()
                .contains("too many paths")
        );
        assert!(add(&snapshot, &[".".into(), "missing".into()]).is_err());
        assert!(index_entries(&snapshot).unwrap().is_empty());
        add(&snapshot, &vec![".".to_owned(); MAX_PATHS]).unwrap();
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
        let snapshot = Snapshot::init(
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
        let snapshot = Snapshot::init(
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
        let snapshot = Snapshot::init(
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
