//! `summary(path)` and `isEmpty(path)`: what a directory holds, in one call.
//! Programs exploring an unfamiliar workspace otherwise spend several runs
//! assembling `exists` / `list` / `stat` loops just to see what is there.
//!
//! Each is gated by `fs.list { path, recursive: false }` alone, checked once up
//! front: the metadata reads that tell a file from a directory, and that date
//! the listed entries, are part of listing, as they are for `ls`. Both reuse
//! the recorded steps `list` takes (open, next, close) and `stat`'s read for the
//! entries; the given path is read following symlinks, as `list` does, and that
//! read is recorded as `fs.stat.follow`.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use wasmtime::{Caller, ExternRef};

use super::{ChargedDirIter, close_handle, effects, next_entry, stat_path};
use crate::runtime::StoreData;
use crate::runtime::filesystem::DirectoryEntry;
use crate::stdlib::shared::check_security_call;

/// Entries `summary` names one per line; the rest are only counted, so a huge
/// directory cannot flood the model's context.
const SHOWN_ENTRIES: usize = 100;

/// `summary(path)`: a few lines of text naming what `path` is and, for a
/// directory, each child with its kind, size and modification time.
pub(super) async fn summarize(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
) -> wasmtime::Result<String> {
    check_list(caller, path)?;
    let Some(stat) = stat_path(caller, path, true).await? else {
        return Ok(format!("{path}: does not exist"));
    };
    if stat.kind != "directory" {
        return Ok(format!(
            "{path}: {}",
            entry_details(&stat.kind, stat.size as u64, Some(stat.modified_ms))
        ));
    }
    let mut counts = Counts::default();
    // Directories first, then by name; only the first `SHOWN_ENTRIES` are kept.
    let mut shown: BTreeMap<(bool, String), DirectoryEntry> = BTreeMap::new();
    // The visitor never stops early, so the walk always runs to the end.
    let _ = for_each_entry(caller, path, |entry| {
        counts.add(&entry);
        shown.insert((entry.kind != "directory", entry.name.clone()), entry);
        if shown.len() > SHOWN_ENTRIES {
            shown.pop_last();
        }
        ControlFlow::Continue(())
    })
    .await?;
    if counts.entries == 0 {
        return Ok(format!("{path}: empty directory"));
    }
    let mut text = format!("{path}: directory, {}", counts.count_line());
    for entry in shown.values() {
        let modified = stat_path(caller, &entry.path, false)
            .await?
            .map(|stat| stat.modified_ms);
        let name = if entry.kind == "directory" {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };
        text.push_str(&format!(
            "\n  {name}  ({})",
            entry_details(entry.kind, entry.bytes, modified)
        ));
    }
    if counts.entries > shown.len() {
        text.push_str(&format!(
            "\n  … {} more not shown; iterate `list({}, false)` for every entry",
            counts.entries - shown.len(),
            serde_json::Value::String(path.to_owned())
        ));
    }
    Ok(text)
}

/// `isEmpty(path)`: a directory with no entries, or a file of zero bytes.
pub(super) async fn is_empty(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
) -> wasmtime::Result<bool> {
    check_list(caller, path)?;
    let Some(stat) = stat_path(caller, path, true).await? else {
        wasmtime::bail!("fs.isEmpty {path}: no such file or directory");
    };
    match stat.kind.as_str() {
        "file" => Ok(stat.size == 0.0),
        "directory" => {
            let walk = for_each_entry(caller, path, |_| ControlFlow::Break(())).await?;
            Ok(walk.is_continue())
        }
        _ => wasmtime::bail!("fs.isEmpty {path}: not a file or directory"),
    }
}

/// The one capability both functions require.
fn check_list(caller: &mut Caller<'_, StoreData>, path: &str) -> wasmtime::Result<()> {
    check_security_call(
        &mut *caller,
        "fs.list",
        serde_json::json!({ "path": path, "recursive": false }),
    )?;
    Ok(())
}

/// Visit the direct children of the directory at `path` until `visit` breaks,
/// closing the listing on every exit as `for…of` does for `list`. Answers
/// whether the walk ran to the end (`Continue`) or `visit` stopped it.
async fn for_each_entry(
    caller: &mut Caller<'_, StoreData>,
    path: &str,
    mut visit: impl FnMut(DirectoryEntry) -> ControlFlow<()>,
) -> wasmtime::Result<ControlFlow<()>> {
    let walk = effects::open_directory(caller, path.to_owned(), false).await?;
    let directory = ChargedDirIter::new(walk, &caller.data().tenant_limits)
        .map_err(|e| wasmtime::Error::new(e).context(format!("fs.list {path}")))?;
    let handle = ExternRef::new(&mut *caller, Some(directory))?;
    let walked: wasmtime::Result<ControlFlow<()>> = async {
        while let Some(entry) = next_entry(caller, handle).await? {
            if visit(entry).is_break() {
                return Ok(ControlFlow::Break(()));
            }
        }
        Ok(ControlFlow::Continue(()))
    }
    .await;
    let closed = close_handle::<ChargedDirIter>(caller, handle).await;
    // The walk's own error explains the failure better than a close after it.
    let walk = walked?;
    closed?;
    Ok(walk)
}

#[derive(Default)]
struct Counts {
    entries: usize,
    files: usize,
    directories: usize,
    symlinks: usize,
    others: usize,
    file_bytes: u64,
}

impl Counts {
    fn add(&mut self, entry: &DirectoryEntry) {
        self.entries += 1;
        match entry.kind {
            "file" => {
                self.files += 1;
                self.file_bytes = self.file_bytes.saturating_add(entry.bytes);
            }
            "directory" => self.directories += 1,
            "symlink" => self.symlinks += 1,
            _ => self.others += 1,
        }
    }

    /// `3 entries: 2 files (14.2 KB), 1 directory`.
    fn count_line(&self) -> String {
        let mut parts = Vec::new();
        if self.files > 0 {
            parts.push(format!(
                "{} ({})",
                plural(self.files, "file", "files"),
                human_size(self.file_bytes)
            ));
        }
        if self.directories > 0 {
            parts.push(plural(self.directories, "directory", "directories"));
        }
        if self.symlinks > 0 {
            parts.push(plural(self.symlinks, "symlink", "symlinks"));
        }
        if self.others > 0 {
            parts.push(plural(self.others, "other", "others"));
        }
        format!(
            "{}: {}",
            plural(self.entries, "entry", "entries"),
            parts.join(", ")
        )
    }
}

/// `file, 12.0 KB, modified 2026-10-20T14:03:11Z`. Only files carry a size.
fn entry_details(kind: &str, bytes: u64, modified_ms: Option<f64>) -> String {
    let mut parts = vec![kind.to_string()];
    if kind == "file" {
        parts.push(human_size(bytes));
    }
    if let Some(modified) = modified_ms.and_then(timestamp) {
        parts.push(format!("modified {modified}"));
    }
    parts.join(", ")
}

/// Whole seconds in UTC; `None` for the zero a backend reports when it has no time.
fn timestamp(modified_ms: f64) -> Option<jiff::Timestamp> {
    if modified_ms <= 0.0 {
        return None;
    }
    jiff::Timestamp::from_second((modified_ms / 1000.0) as i64).ok()
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_the_largest_whole_unit() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(12 * 1024 + 205), "12.2 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn counts_name_each_kind_present() {
        let entry = |kind: &'static str, bytes| DirectoryEntry {
            name: String::new(),
            path: String::new(),
            kind,
            bytes,
        };
        let mut counts = Counts::default();
        counts.add(&entry("file", 2048));
        counts.add(&entry("file", 0));
        counts.add(&entry("directory", 0));
        counts.add(&entry("symlink", 0));
        assert_eq!(
            counts.count_line(),
            "4 entries: 2 files (2.0 KB), 1 directory, 1 symlink"
        );
    }
}
