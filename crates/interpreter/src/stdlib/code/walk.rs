//! Ignore-aware traversal uses VFS handles only; ignore matchers see virtual paths.
use super::{
    argument,
    budget::{Budget, OutputBudget},
    gate, normalize, read_contents, read_file, text,
};
use crate::runtime::fs::resolve_link;
use crate::runtime::fuel;
use crate::runtime::host::invariant_trap;
use crate::runtime::{
    StoreData,
    host::read_boxed_number,
    prelude::collection::{object_field, read_array_vals, unbox_bool},
};
use crate::stdlib::shared::{contain_trap, resolve_content_or_trap, resolve_link_or_trap};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};
use std::{collections::BinaryHeap, path::Path, time::UNIX_EPOCH};
use wasmtime::{Caller, Result, Val, bail};
const MAX_RESULTS: usize = 1000;
const MAX_ENTRIES: usize = 20_000;

struct Entry {
    path: String,
    kind: &'static str,
    depth: usize,
    modified: f64,
}
impl Entry {
    fn value(&self) -> Value {
        json!({"path":self.path,"kind":self.kind,"depth":self.depth,"modifiedAt":self.modified})
    }
}
struct Directory {
    path: String,
    depth: usize,
    ignores: IgnoreHeads,
}

struct OrderedDirectory(Directory);
impl PartialEq for OrderedDirectory {
    fn eq(&self, other: &Self) -> bool {
        self.0.path == other.0.path
    }
}
impl Eq for OrderedDirectory {}
impl PartialOrd for OrderedDirectory {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedDirectory {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Descendants start at `path + '/'`; a sibling `a-` precedes `a/`.
        other
            .0
            .path
            .bytes()
            .chain(std::iter::once(b'/'))
            .cmp(self.0.path.bytes().chain(std::iter::once(b'/')))
    }
}

struct OrderedEntry(Entry);
impl PartialEq for OrderedEntry {
    fn eq(&self, other: &Self) -> bool {
        self.0.path == other.0.path
    }
}
impl Eq for OrderedEntry {}
impl PartialOrd for OrderedEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.path.cmp(&other.0.path)
    }
}

#[derive(Default)]
struct PendingDirectories {
    stack: Vec<Directory>,
    ordered: BinaryHeap<OrderedDirectory>,
}
impl PendingDirectories {
    fn push(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        dir: Directory,
        tree: bool,
    ) -> Result<()> {
        if tree {
            let log = u64::from((self.ordered.len() as u64 + 1).max(2).ilog2()) + 1;
            fuel::charge_host_fuel(
                &mut *caller,
                fuel::ELEM
                    .cost(log)
                    .saturating_add(fuel::SCAN.cost((dir.path.len() as u64).saturating_mul(log))),
            )?;
            self.ordered
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            self.ordered.push(OrderedDirectory(dir));
        } else {
            self.stack
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            self.stack.push(dir);
        }
        Ok(())
    }
    fn pop(&mut self, cutoff: Option<&str>) -> Option<Directory> {
        if let Some(next) = self.ordered.peek() {
            if cutoff.is_some_and(|cutoff| {
                next.0
                    .path
                    .bytes()
                    .chain(std::iter::once(b'/'))
                    .cmp(cutoff.bytes())
                    .is_ge()
            }) {
                return None;
            }
            return self.ordered.pop().map(|dir| dir.0);
        }
        self.stack.pop()
    }
}

#[derive(Default)]
struct WalkEntries {
    all: Vec<Entry>,
    lowest: BinaryHeap<OrderedEntry>,
}
impl WalkEntries {
    fn cutoff(&self) -> Option<&str> {
        (self.lowest.len() > MAX_RESULTS)
            .then(|| self.lowest.peek().map(|entry| entry.0.path.as_str()))
            .flatten()
    }
    fn push(&mut self, caller: &mut Caller<'_, StoreData>, entry: Entry, tree: bool) -> Result<()> {
        if !tree {
            self.all
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            self.all.push(entry);
            return Ok(());
        }
        let log = u64::from((self.lowest.len() as u64 + 1).max(2).ilog2()) + 1;
        fuel::charge_host_fuel(
            &mut *caller,
            fuel::ELEM
                .cost(log)
                .saturating_add(fuel::SCAN.cost((entry.path.len() as u64).saturating_mul(log))),
        )?;
        if self.lowest.len() > MAX_RESULTS {
            let mut largest = self.lowest.peek_mut().ok_or_else(|| {
                crate::runtime::host::fatal_host_error("code.tree: missing cutoff entry")
            })?;
            if entry.path < largest.0.path {
                *largest = OrderedEntry(entry);
            }
        } else {
            self.lowest
                .try_reserve(1)
                .map_err(crate::runtime::host::fatal_host_error)?;
            self.lowest.push(OrderedEntry(entry));
        }
        Ok(())
    }
    fn into_vec(self, tree: bool) -> Result<Vec<Entry>> {
        if !tree {
            return Ok(self.all);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.lowest.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        entries.extend(self.lowest.into_vec().into_iter().map(|entry| entry.0));
        Ok(entries)
    }
}
pub(super) fn tree(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    root: &str,
    depth: usize,
) -> Result<Value> {
    let entries = walk(
        caller,
        budget,
        WalkStart {
            path: root,
            depth: 0,
            origin: root,
        },
        depth,
        "tree",
    )?;
    listing(caller, budget, &entries)
}
pub(super) fn glob(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    pattern: &str,
) -> Result<Value> {
    let absolute = pattern.starts_with('/');
    let cwd = if absolute {
        "/"
    } else {
        caller.data().vfs.cwd()
    }
    .to_owned();
    let prefix_root = glob_root(pattern);
    let root = format!("{}{}", cwd.trim_end_matches('/'), prefix_root);
    let pattern = GlobBuilder::new(pattern.trim_start_matches('/'))
        .literal_separator(true)
        .build()?
        .compile_matcher();
    let root_depth = prefix_root
        .split('/')
        .filter(|part| !part.is_empty())
        .count();
    let mut entries = walk(
        caller,
        budget,
        WalkStart {
            path: &root,
            depth: root_depth,
            origin: &cwd,
        },
        usize::MAX,
        "glob",
    )?;
    let path_bytes = entries
        .iter()
        .filter(|entry| entry.kind == "file")
        .fold(0_u64, |sum, entry| {
            sum.saturating_add(entry.path.len() as u64)
        });
    fuel::charge(&mut *caller, fuel::SCAN, path_bytes)?;
    let prefix = format!("{}/", cwd.trim_end_matches('/'));
    entries.retain(|e| {
        e.kind == "file" && pattern.is_match(e.path.strip_prefix(&prefix).unwrap_or(&e.path))
    });
    fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(entries.len() as u64))?;
    entries.sort_unstable_by(|a, b| {
        b.modified
            .total_cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    listing(caller, budget, &entries)
}
fn glob_root(pattern: &str) -> String {
    let mut root = String::new();
    let mut components = pattern.trim_start_matches('/').split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none()
            || component.is_empty()
            || matches!(component, "." | "..")
            || component.contains(['*', '?', '[', '{', '\\'])
        {
            break;
        }
        root.push('/');
        root.push_str(component);
    }
    if root.is_empty() { "/".into() } else { root }
}

pub(super) fn search(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    pattern: &str,
    options: &Val,
) -> Result<Value> {
    let options = Options::read(caller, budget, options)?;
    budget.charge(caller, 2 * 1024 * 1024)?;
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(!options.sensitive)
        .size_limit(1024 * 1024)
        .dfa_size_limit(1024 * 1024)
        .build()?;
    let entries = walk(
        caller,
        budget,
        WalkStart {
            path: &options.path,
            depth: 0,
            origin: &options.path,
        },
        usize::MAX,
        "search",
    )?;
    let mut results = SearchResults::new(&options);
    for entry in entries {
        if entry.kind != "file" || !options.includes_path(&entry.path) {
            continue;
        }
        search_file(caller, budget, &entry.path, &regex, &mut results)?;
        if results.truncated {
            break;
        }
    }
    Ok(results.into_value())
}

fn search_file(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    path: &str,
    regex: &Regex,
    results: &mut SearchResults<'_>,
) -> Result<()> {
    let mut file_budget = Budget::new(caller);
    let source = read_contents(caller, &mut file_budget, path, "search", true)?;
    file_budget.charge(
        caller,
        source
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            .saturating_add(1)
            .saturating_mul(32),
    )?;
    let lines = text::lines(source.strip_prefix('\u{feff}').unwrap_or(&source));
    fuel::charge(
        &mut *caller,
        fuel::SCAN,
        (source.len() as u64).saturating_mul(regex.as_str().len().max(1) as u64),
    )?;
    let mut count = 0;
    for (index, line) in lines.iter().enumerate() {
        if !regex.is_match(text::line_text(line)) {
            continue;
        }
        count += 1;
        results.record_match(caller, budget, path, &lines, index)?;
        if results.truncated {
            return Ok(());
        }
    }
    results.record_file(caller, budget, path, count)
}

struct SearchResults<'a> {
    options: &'a Options,
    matches: Vec<Value>,
    files: Vec<String>,
    counts: Vec<Value>,
    output: OutputBudget,
    truncated: bool,
}
impl<'a> SearchResults<'a> {
    fn new(options: &'a Options) -> Self {
        Self {
            options,
            matches: Vec::new(),
            files: Vec::new(),
            counts: Vec::new(),
            output: OutputBudget::new(),
            truncated: false,
        }
    }
    fn into_value(self) -> Value {
        json!({"matches":self.matches,"files":self.files,"counts":self.counts,"truncated":self.truncated})
    }
    fn record_match(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        budget: &mut Budget,
        path: &str,
        lines: &[&str],
        index: usize,
    ) -> Result<()> {
        if self.options.mode != SearchMode::Matches {
            return Ok(());
        }
        let start = index.saturating_sub(self.options.context);
        let line = lines
            .get(index)
            .ok_or_else(|| invariant_trap("code: search line out of bounds"))?;
        let after = index
            .checked_add(1)
            .ok_or_else(|| invariant_trap("code: search line overflow"))?;
        let end = after.saturating_add(self.options.context).min(lines.len());
        let context = lines
            .get(start..end)
            .ok_or_else(|| invariant_trap("code: search context out of bounds"))?;
        let bytes = context
            .iter()
            .fold(OutputBudget::line_bytes(path), |sum, line| {
                sum.saturating_add(OutputBudget::line_bytes(line))
            });
        if !self.reserve(caller, budget, bytes)? {
            return Ok(());
        }
        self.matches.push(json!({"path":path,"line":after,"text":text::line_text(line),"before":text::numbered(lines,start,index),"after":text::numbered(lines,after,end)}));
        Ok(())
    }
    fn record_file(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        budget: &mut Budget,
        path: &str,
        count: usize,
    ) -> Result<()> {
        if count == 0 || self.options.mode == SearchMode::Matches {
            return Ok(());
        }
        if !self.reserve(caller, budget, OutputBudget::line_bytes(path))? {
            return Ok(());
        }
        match self.options.mode {
            SearchMode::Files => self.files.push(path.into()),
            SearchMode::Counts => self.counts.push(json!({"path":path,"count":count})),
            SearchMode::Matches => {
                return Err(invariant_trap("code: invalid per-file search mode"));
            }
        }
        Ok(())
    }
    fn reserve(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        budget: &mut Budget,
        bytes: usize,
    ) -> Result<bool> {
        let count = self.matches.len() + self.files.len() + self.counts.len();
        if count == self.options.limit || !self.output.reserve(caller, budget, bytes)? {
            self.truncated = true;
            return Ok(false);
        }
        Ok(true)
    }
}

struct WalkStart<'a> {
    path: &'a str,
    depth: usize,
    origin: &'a str,
}

fn walk(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    start: WalkStart<'_>,
    depth: usize,
    op: &str,
) -> Result<Vec<Entry>> {
    let root = start.path;
    if !validate_root(caller, root, op, start.origin)? {
        return Ok(Vec::new());
    }
    let tree = op == "tree";
    let mut entries = WalkEntries::default();
    let mut rules = IgnoreRules::default();
    let Some(inherited) = ancestor_ignores(caller, budget, root, op, start.origin, &mut rules)?
    else {
        return Ok(Vec::new());
    };
    let mut pending = PendingDirectories::default();
    pending.push(
        caller,
        Directory {
            path: root.into(),
            depth: start.depth,
            ignores: inherited,
        },
        tree,
    )?;
    let mut visited = 0;
    while let Some(mut dir) = pending.pop(entries.cutoff()) {
        fuel::charge(&mut *caller, fuel::SYSCALL, 1)?;
        if dir.depth >= depth {
            continue;
        }
        gate(caller, "fs.list", &dir.path)?;
        load_ignores(
            caller,
            budget,
            &dir.path,
            IgnoreScope::Traversed,
            op,
            &mut dir.ignores,
            &mut rules,
        )?;
        let resolved = resolve_content_or_trap(caller.data(), &dir.path, op)?;
        for item in resolved
            .entries()
            .map_err(|e| contain_trap(op, &dir.path, &e))?
        {
            let item = item?;
            visited += 1;
            if visited > MAX_ENTRIES {
                bail!("code.{op}: traversal exceeds {MAX_ENTRIES} entries; choose a smaller root");
            }
            fuel::charge(&mut *caller, fuel::SYSCALL, 1)?;
            let Some(entry) = inspect_entry(caller, budget, &item, &dir, &rules, op)? else {
                continue;
            };
            if entry.kind == "directory" {
                budget.charge(caller, entry.path.len())?;
                pending.push(
                    caller,
                    Directory {
                        path: entry.path.clone(),
                        depth: entry.depth,
                        ignores: dir.ignores,
                    },
                    tree,
                )?;
            }
            entries.push(caller, entry, tree)?;
        }
    }
    let mut entries = entries.into_vec(tree)?;
    fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(entries.len() as u64))?;
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
fn validate_root(
    caller: &mut Caller<'_, StoreData>,
    root: &str,
    op: &str,
    origin: &str,
) -> Result<bool> {
    // `/` has no components to stat; the walk's fs.list gate covers it.
    if root != "/" {
        gate(caller, "fs.stat", root)?;
    }
    // Ancestors are inspected without a policy check, so a grant narrowed to
    // the root still works. Errors name the root, never an ancestor, so the
    // check discloses nothing about paths outside the grant.
    let mut path = String::new();
    for component in root.split('/').filter(|component| !component.is_empty()) {
        path.push('/');
        path.push_str(component);
        if op == "glob"
            && path != origin
            && Path::new(&path).starts_with(origin)
            && component.starts_with('.')
        {
            return Ok(false);
        }
        let metadata = match resolve_link(&caller.data().vfs, caller.data().vfs.cwd(), &path)
            .and_then(|resolved| resolved.symlink_metadata())
        {
            Ok(metadata) => metadata,
            Err(crate::runtime::fs::ContainError::Io(error))
                if op == "glob" && error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(false);
            }
            Err(error) => return Err(contain_trap(op, root, &error)),
        };
        if op == "glob" && !metadata.is_dir() {
            return Ok(false);
        }
        if metadata.file_type().is_symlink() {
            bail!("code.{op}: navigation root {root} must not traverse a symlink");
        }
    }
    Ok(true)
}
fn ancestor_ignores(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    root: &str,
    op: &str,
    origin: &str,
    rules: &mut IgnoreRules,
) -> Result<Option<IgnoreHeads>> {
    let mut parents = Vec::new();
    let mut parent = Path::new(root).parent();
    while let Some(path) = parent {
        parents.push(path.to_string_lossy().into_owned());
        parent = path.parent();
    }
    let mut inherited = IgnoreHeads::default();
    for parent in parents.into_iter().rev() {
        if op == "glob"
            && parent != origin
            && Path::new(&parent).starts_with(origin)
            && rules.ignored(caller, inherited, &parent, true)?
        {
            return Ok(None);
        }
        load_ignores(
            caller,
            budget,
            &parent,
            IgnoreScope::Ancestor,
            op,
            &mut inherited,
            rules,
        )?;
    }
    if op == "glob"
        && Path::new(root) != Path::new(origin)
        && rules.ignored(caller, inherited, root, true)?
    {
        return Ok(None);
    }
    Ok(Some(inherited))
}
fn inspect_entry(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    item: &cap_std::fs::DirEntry,
    dir: &Directory,
    rules: &IgnoreRules,
    op: &str,
) -> Result<Option<Entry>> {
    let raw = item.file_name();
    let Some(name) = raw.to_str() else {
        bail!("code.{op}: non-UTF-8 filename in {}", dir.path);
    };
    if name.starts_with('.') {
        return Ok(None);
    }
    let path = format!("{}/{}", dir.path.trim_end_matches('/'), name);
    budget.charge(caller, path.len() + 256)?;
    gate(caller, "fs.stat", &path)?;
    let link = resolve_link_or_trap(caller.data(), &path, op)?;
    let metadata = link
        .symlink_metadata()
        .map_err(|e| contain_trap(op, &path, &e))?;
    let kind = crate::stdlib::fs::handles::kind_of(&metadata.file_type());
    if rules.ignored(caller, dir.ignores, &path, kind == "directory")? {
        return Ok(None);
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.into_std().duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |t| t.as_secs_f64() * 1000.0);
    Ok(Some(Entry {
        path,
        kind,
        depth: dir.depth + 1,
        modified,
    }))
}
#[derive(Clone, Copy, Default)]
struct IgnoreHeads {
    gitignore: Option<usize>,
    custom: Option<usize>,
}

struct IgnoreLayer {
    matcher: Gitignore,
    parent: Option<usize>,
}

// Indices share inherited chains without recursive ownership or drop.
#[derive(Default)]
struct IgnoreRules {
    layers: Vec<IgnoreLayer>,
}
impl IgnoreRules {
    fn push(&mut self, head: &mut Option<usize>, matcher: Gitignore) -> Result<()> {
        self.layers
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        let index = self.layers.len();
        self.layers.push(IgnoreLayer {
            matcher,
            parent: *head,
        });
        *head = Some(index);
        Ok(())
    }

    fn ignored(
        &self,
        caller: &mut Caller<'_, StoreData>,
        heads: IgnoreHeads,
        path: &str,
        directory: bool,
    ) -> Result<bool> {
        // Every .ignore takes precedence over every .gitignore, regardless of depth.
        for mut head in [heads.custom, heads.gitignore] {
            while let Some(index) = head {
                let layer = self.layers.get(index).ok_or_else(|| {
                    crate::runtime::host::fatal_host_error("code: invalid ignore rule index")
                })?;
                let patterns = layer
                    .matcher
                    .num_ignores()
                    .saturating_add(layer.matcher.num_whitelists())
                    .max(1);
                fuel::charge(
                    &mut *caller,
                    fuel::SCAN,
                    patterns.saturating_mul(path.len() as u64),
                )?;
                let result = layer.matcher.matched(path, directory);
                if !result.is_none() {
                    return Ok(result.is_ignore());
                }
                head = layer.parent;
            }
        }
        Ok(false)
    }
}
/// Where an ignore file sits relative to the traversal root.
#[derive(Clone, Copy)]
enum IgnoreScope {
    /// Above the root: consulted for its rules, outside what the caller asked for.
    Ancestor,
    /// The root or a directory the walk lists.
    Traversed,
}
fn load_ignores(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    dir: &str,
    scope: IgnoreScope,
    op: &str,
    ignores: &mut IgnoreHeads,
    rules: &mut IgnoreRules,
) -> Result<()> {
    for name in [".gitignore", ".ignore"] {
        let path = format!("{}/{}", dir.trim_end_matches('/'), name);
        if !may_probe_ignore(caller, &path, scope)? {
            continue;
        }
        let resolved = resolve_link_or_trap(caller.data(), &path, op)?;
        let metadata = match resolved.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(crate::runtime::fs::ContainError::Io(e))
                if e.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(e) => return Err(contain_trap(op, &path, &e)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let source = read_file(caller, budget, &path, op)?;
        budget.charge(caller, source.len().saturating_mul(32))?;
        let mut builder = GitignoreBuilder::new(dir);
        for line in source.strip_prefix('\u{feff}').unwrap_or(&source).lines() {
            builder.add_line(Some(path.clone().into()), line)?;
        }
        let head = if name == ".ignore" {
            &mut ignores.custom
        } else {
            &mut ignores.gitignore
        };
        budget.charge(caller, 2 * std::mem::size_of::<IgnoreLayer>())?;
        rules.push(head, builder.build()?)?;
    }
    Ok(())
}
/// Whether an ignore file may be looked for. Inside the root, an fs.stat
/// denial fails the call. Ancestors need read access, the only reason to touch
/// them, and a grant narrowed to the root need not cover them: a policy denial
/// means the file is not consulted, and since nothing is read, nothing about it
/// leaks. Only the policy's own answer filters; an invariant denial means the
/// check could not be made and still fails the call.
fn may_probe_ignore(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
    scope: IgnoreScope,
) -> Result<bool> {
    match scope {
        IgnoreScope::Traversed => {
            gate(caller, "fs.stat", path)?;
            Ok(true)
        }
        IgnoreScope::Ancestor => {
            let Err(err) = gate(caller, "fs.read", path) else {
                return Ok(true);
            };
            match err.downcast_ref::<crate::runtime::host::PermissionDenied>() {
                Some(denial) if denial.is_policy() => Ok(false),
                _ => Err(err),
            }
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Matches,
    Files,
    Counts,
}

struct Options {
    path: String,
    include: GlobSet,
    exclude: GlobSet,
    sensitive: bool,
    context: usize,
    limit: usize,
    mode: SearchMode,
}
impl Options {
    fn includes_path(&self, path: &str) -> bool {
        let relative = path
            .strip_prefix(self.path.trim_end_matches('/'))
            .unwrap_or(path)
            .trim_start_matches('/');
        (self.include.is_empty() || self.include.is_match(relative))
            && !self.exclude.is_match(relative)
    }
    fn read(caller: &mut Caller<'_, StoreData>, budget: &mut Budget, val: &Val) -> Result<Self> {
        let path = match present(caller, val, "path")? {
            Some(v) => {
                let path = argument(caller, budget, &v)?;
                normalize(caller.data().vfs.cwd(), &path)?
            }
            None => caller.data().vfs.cwd().to_owned(),
        };
        let mode = match present(caller, val, "mode")? {
            Some(v) => argument(caller, budget, &v)?,
            None => "matches".into(),
        };
        let mode = match mode.as_str() {
            "matches" => SearchMode::Matches,
            "files" => SearchMode::Files,
            "counts" => SearchMode::Counts,
            _ => bail!("code.search: mode must be matches, files, or counts"),
        };
        let sensitive = match present(caller, val, "caseSensitive")? {
            Some(v) => unbox_bool(caller, &v)?,
            None => true,
        };
        let context = option_number(caller, val, "context", 0, 0, 1000)?;
        let limit = option_number(caller, val, "limit", MAX_RESULTS, 1, MAX_RESULTS)?;
        Ok(Self {
            path,
            mode,
            sensitive,
            context,
            limit,
            include: patterns(caller, budget, val, "include")?,
            exclude: patterns(caller, budget, val, "exclude")?,
        })
    }
}
fn present(caller: &mut Caller<'_, StoreData>, obj: &Val, field: &str) -> Result<Option<Val>> {
    Ok(object_field(caller, obj, field)?.filter(|v| !matches!(v, Val::AnyRef(None))))
}
fn option_number(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize> {
    let Some(v) = present(caller, obj, name)? else {
        return Ok(default);
    };
    let n = read_boxed_number(caller, &v, name)?;
    if !n.is_finite() || n.fract() != 0.0 || n < min as f64 || n > max as f64 {
        bail!("code.search: {name} must be an integer between {min} and {max}");
    }
    Ok(n as usize)
}
fn patterns(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    obj: &Val,
    field: &str,
) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    if let Some(v) = present(caller, obj, field)? {
        for val in read_array_vals(caller, &v)? {
            builder.add(
                GlobBuilder::new(&argument(caller, budget, &val)?)
                    .literal_separator(true)
                    .build()?,
            );
        }
    }
    Ok(builder.build()?)
}

fn listing(
    caller: &mut Caller<'_, StoreData>,
    budget: &mut Budget,
    entries: &[Entry],
) -> Result<Value> {
    let mut output = OutputBudget::new();
    let mut values = Vec::new();
    for entry in entries.iter().take(MAX_RESULTS) {
        if !output.reserve(caller, budget, OutputBudget::line_bytes(&entry.path))? {
            break;
        }
        values.push(entry.value());
    }
    Ok(json!({"truncated":values.len() < entries.len(),"entries":values}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_ignore_storage_grows_linearly() {
        for count in [128, 256] {
            let mut rules = IgnoreRules::default();
            let mut head = None;
            let mut directories = Vec::new();
            for _ in 0..count {
                let mut builder = GitignoreBuilder::new("/");
                builder.add_line(None, "*.tmp").unwrap();
                rules.push(&mut head, builder.build().unwrap()).unwrap();
                directories.push(IgnoreHeads {
                    gitignore: head,
                    custom: None,
                });
            }
            assert_eq!(rules.layers.len(), count);
            assert_eq!(directories.len(), count);
            let old_copies = count * (count + 1) / 2;
            assert!(rules.layers.len() < old_copies / 2);
            let mut visited = 0;
            while let Some(index) = head {
                let layer = &rules.layers[index];
                head = layer.parent;
                visited += 1;
            }
            assert_eq!(visited, count);
        }
    }

    #[test]
    fn glob_prefix_stops_before_patterns_and_escaped_components() {
        for (pattern, root) in [
            ("src/nested/*.ts", "/src/nested"),
            ("/src/**/*.ts", "/src"),
            ("src/file.ts", "/src"),
            ("src/{a,b}/file.ts", "/src"),
            ("src/a\\*/file.ts", "/src"),
            ("*.ts", "/"),
            ("../*.ts", "/"),
        ] {
            assert_eq!(glob_root(pattern), root);
        }
    }
}
