//! Externref payloads for `submilli:fs` open resources.
//!
//! Each payload owns its OS handle plus a clone of `TenantLimits.host_attached_bytes`
//! (via [`ByteCharge`]); it charges the in-process buffer footprint at creation and
//! refunds it when wasmtime GC reclaims the externref.
//!
//! Readers `close()` eagerly: the OS handle is dropped (freeing the fd immediately) and
//! the bytes refunded; `Drop` is the safety net for a reader that was never closed.
//! A [`ChargedFileWriter`]'s explicit `close()` performs the atomic temp→final rename;
//! its `Drop` only flushes and removes the temp file, because a GC-timed `rename()` would
//! be non-deterministic.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cap_std::fs::{Dir, DirEntry, ReadDir};

use crate::runtime::fs::{ContainError, ContentPath, FileIdentity};
use crate::runtime::limits::{MemoryCapExceeded, TenantLimits};
use crate::runtime::{DiskQuota, Holder, OpenFileGuard, QuotaCharge, QuotaExceeded};

/// Footprint charged per reader/writer, matching the default `BufReader`/`BufWriter`
/// capacity. Byte readers add their `chunk_size` working buffer on top.
const HANDLE_BUF_BYTES: u64 = 8 * 1024;

/// Bytes charged against the store's host-attached counter, refunded on `release`/`drop`.
struct ByteCharge {
    bytes: u64,
    counter: Arc<AtomicU64>,
}

impl ByteCharge {
    fn new(limits: &TenantLimits, bytes: u64) -> Result<Self, MemoryCapExceeded> {
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }

    /// Refunds the charged bytes once; subsequent calls are no-ops.
    fn release(&mut self) {
        let n = std::mem::take(&mut self.bytes);
        if n == 0 {
            return;
        }
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(n))
            });
    }
}

impl Drop for ByteCharge {
    fn drop(&mut self) {
        self.release();
    }
}

/// An iterator handle with an idempotent eager close — what the shared
/// `for…of` close path calls on any of the three reader kinds.
pub trait Closable: Send + Sync {
    fn close(&mut self);
}

/// Line iterator over a file. `close()` and `Drop` both free the fd and refund bytes.
pub struct ChargedLineReader {
    reader: Option<BufReader<File>>,
    buf: Vec<u8>,
    bom_handled: bool,
    charge: ByteCharge,
    /// Keeps the file's bytes counted against the size limit while it is open,
    /// even after its name is removed.
    held: Option<OpenFileGuard>,
}

impl ChargedLineReader {
    pub fn new(
        reader: BufReader<File>,
        limits: &TenantLimits,
        held: Option<OpenFileGuard>,
    ) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            reader: Some(reader),
            buf: Vec::new(),
            bom_handled: false,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
            held,
        })
    }

    /// Next line with the trailing `\n`/`\r\n` stripped; a leading UTF-8 BOM is dropped
    /// from the first line. `Ok(None)` at EOF or once closed.
    pub fn read_next(&mut self) -> std::io::Result<Option<String>> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(None);
        };
        self.buf.clear();
        let n = reader.read_until(b'\n', &mut self.buf)?;
        if n == 0 {
            return Ok(None);
        }
        if self.buf.last() == Some(&b'\n') {
            self.buf.pop();
            if self.buf.last() == Some(&b'\r') {
                self.buf.pop();
            }
        }
        let mut text = String::from_utf8_lossy(&self.buf).into_owned();
        if !self.bom_handled {
            self.bom_handled = true;
            if text.starts_with('\u{FEFF}') {
                text.remove(0);
            }
        }
        Ok(Some(text))
    }

    pub fn close(&mut self) {
        self.reader = None;
        self.charge.release();
        self.held = None;
    }
}

/// Fixed-size chunk iterator over a file.
pub struct ChargedByteReader {
    file: Option<File>,
    chunk_size: usize,
    charge: ByteCharge,
    /// Keeps the file's bytes counted against the size limit while it is open,
    /// even after its name is removed.
    held: Option<OpenFileGuard>,
}

impl ChargedByteReader {
    pub fn new(
        file: File,
        chunk_size: usize,
        limits: &TenantLimits,
        held: Option<OpenFileGuard>,
    ) -> Result<Self, MemoryCapExceeded> {
        let bytes = HANDLE_BUF_BYTES.saturating_add(chunk_size as u64);
        Ok(Self {
            file: Some(file),
            chunk_size,
            charge: ByteCharge::new(limits, bytes)?,
            held,
        })
    }

    /// Next chunk of up to `chunk_size` bytes; the final chunk may be short.
    /// `Ok(None)` at EOF or once closed.
    pub fn read_next(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let Some(file) = self.file.as_mut() else {
            return Ok(None);
        };
        let mut buf = vec![0u8; self.chunk_size];
        let mut filled = 0;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            Ok(None)
        } else {
            buf.truncate(filled);
            Ok(Some(buf))
        }
    }

    pub fn close(&mut self) {
        self.file = None;
        self.charge.release();
        self.held = None;
    }
}

/// One directory entry as the guest sees it. Guest-relative path included, because the
/// walk knows it — deriving it afterwards by stripping a host prefix is what leaked host
/// paths into responses.
pub struct WalkEntry {
    pub name: String,
    pub guest_path: String,
    pub kind: &'static str,
    pub size: u64,
}

/// Concurrently open directory handles per walk. A deep tree times the number of list
/// handles a guest holds open consumes process-wide descriptors that the tenant limiter
/// does not account for, since it charges a byte budget only. Subdirectories found past
/// the cap are postponed and reopened from the base handle once a handle frees up.
///
/// Higher than `walkdir`'s `max_open` default of 10: `walkdir` pays only extra syscalls
/// when it hits the cap (it closes the *shallowest* level and reopens it by path,
/// preserving depth-first order), whereas postponing costs ordering as well — a postponed
/// subtree lands after the rest of its parent's entries. Real checkouts (nested
/// `node_modules`, deep monorepos) routinely pass 10 levels and rarely pass 32.
///
/// The `list` doc in `declaration.rs` states this depth as the guest-facing ordering
/// contract — change both together.
const MAX_OPEN_DIRS: usize = 32;

/// Postponed subdirectory names a single walk will hold at once. Only levels past
/// [`MAX_OPEN_DIRS`] postpone at all, so reaching this needs a directory 32 deep with a
/// fan-out in the thousands; past it a subdirectory is skipped rather than remembered,
/// which is the same best-effort outcome an unreadable one already gets. Without a
/// ceiling the queue grows with the width of a directory the guest did not have to pay
/// for in the byte cap.
const MAX_DEFERRED: usize = 16_384;

struct Level {
    /// `None` once the directory is fully read: the handle is released at that point,
    /// while the frame stays on the stack until [`Level::deferred`] drains.
    iter: Option<ReadDir>,
    /// Path of this level's directory relative to the listed directory — empty at the top.
    /// Base-relative rather than guest-absolute so a postponed level can be reopened
    /// against the base handle, and a `PathBuf` rather than a `String` so a name the
    /// platform allows but UTF-8 does not still reopens — the lossy [`Level::prefix`]
    /// would have replaced it with U+FFFD and lost the subtree.
    path: PathBuf,
    /// Lossy rendering of [`Level::path`] with a trailing separator: `""` at the top,
    /// `"sub/"` one down. Guest paths are built from this.
    prefix: String,
    /// Names of subdirectories of this level whose descent was postponed — because the
    /// open-handle cap was reached, or because opening them from this level's handle
    /// failed (most likely descriptor exhaustion, which is exactly what the cap manages).
    /// Drained in order once this level is fully read, each rejoined to [`Level::path`]
    /// and reopened from the base handle.
    ///
    /// Bare names, not paths: at the depth where postponement starts, the level's own
    /// path is 32 components long, and storing it again per postponed child would make
    /// the queue tens of times larger than the entry names it stands for.
    deferred: VecDeque<OsString>,
}

impl Level {
    fn new(path: PathBuf, iter: ReadDir) -> Self {
        let prefix = if path.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", path.to_string_lossy())
        };
        Self {
            iter: Some(iter),
            path,
            prefix,
            deferred: VecDeque::new(),
        }
    }
}

/// Directory walker (flat or recursive), rooted at a contained handle.
///
/// Lazy, pre-order, and non-following — matching `walkdir`'s default, so a checkout's
/// internal links surface as entries rather than being descended and double-walked.
/// Per-entry errors skip and continue rather than trapping the whole iteration: a
/// recursive walk over a real checkout is the flagship use case, and one unreadable
/// subtree should not end it.
///
/// Ordering: every directory is yielded before its descendants, and every subtree is
/// yielded as one contiguous run — nothing outside a directory's subtree is interleaved
/// into it. Strictly depth-first down to [`MAX_OPEN_DIRS`] levels of nesting; past that,
/// a subdirectory's own contents follow the rest of its parent's entries instead of
/// coming immediately after it, because the descent has to wait for a directory handle.
pub struct ContainedWalk {
    base: Arc<Dir>,
    /// Mount points below the base, relative to it, with the volume handle a
    /// descent opens in place of the empty directory the root holds there.
    mounts: Vec<(PathBuf, Arc<Dir>)>,
    /// Guest-relative path of the listed directory, `""` for the root, else `"tree/"`.
    base_prefix: String,
    stack: Vec<Level>,
    /// Levels on `stack` still holding a `ReadDir`; the frames that have released theirs
    /// stay on the stack while their postponed subdirectories drain.
    open_dirs: usize,
    /// Postponed names held across every level, against [`MAX_DEFERRED`].
    deferred: usize,
    recursive: bool,
}

impl ContainedWalk {
    /// `base_prefix` is the guest-relative path of the listed directory, `""` for the root.
    /// `mounts` are the mount points below it; see [`ContentPath::mounts_below`].
    pub fn new(
        base: Arc<Dir>,
        base_prefix: String,
        recursive: bool,
        mounts: Vec<(PathBuf, Arc<Dir>)>,
    ) -> std::io::Result<Self> {
        let iter = base.entries()?;
        Ok(Self {
            base,
            mounts,
            base_prefix,
            stack: vec![Level::new(PathBuf::new(), iter)],
            open_dirs: 1,
            deferred: 0,
            recursive,
        })
    }

    fn next_entry(&mut self) -> Option<WalkEntry> {
        loop {
            let level = self.stack.last_mut()?;
            match level.iter.as_mut().map(Iterator::next) {
                Some(Some(Ok(entry))) => {
                    if let Some(found) = self.visit(entry) {
                        return Some(found);
                    }
                }
                Some(Some(Err(_))) => {}
                Some(None) => {
                    level.iter = None;
                    self.open_dirs -= 1;
                }
                None => self.resume_deferred(),
            }
        }
    }

    /// Yields `entry`, descending into it first when it is a directory a recursive walk
    /// should enter. `None` for an entry whose type cannot be determined at all.
    fn visit(&mut self, entry: DirEntry) -> Option<WalkEntry> {
        let raw_name = entry.file_name();
        // `file_type` can fail where the directory entry carries no type; the entry's
        // own metadata is the fallback before giving up on it.
        let ft = entry
            .file_type()
            .ok()
            .or_else(|| entry.metadata().ok().map(|m| m.file_type()))?;
        let size = if ft.is_file() {
            entry.metadata().map_or(0, |m| m.len())
        } else {
            0
        };
        let level = self.stack.last()?;
        let name = raw_name.to_string_lossy().into_owned();
        let guest_path = format!("/{}{}{}", self.base_prefix, level.prefix, name);
        if self.recursive && ft.is_dir() && !ft.is_symlink() {
            self.descend(&raw_name, &entry);
        }
        Some(WalkEntry {
            name,
            guest_path,
            kind: kind_of(&ft),
            size,
        })
    }

    /// Opens the subdirectory named `name` as the new innermost level, or postpones it on
    /// the current level — when there is no handle to spare, and equally when the open
    /// fails, since a failure here is most likely descriptor exhaustion and dropping the
    /// subtree over that would silently truncate the walk. Past [`MAX_DEFERRED`] it is
    /// dropped after all, the walk having run out of room to remember it.
    fn descend(&mut self, name: &OsStr, entry: &DirEntry) {
        if self.open_dirs < MAX_OPEN_DIRS
            && let Some(child) = self.stack.last().map(|level| level.path.join(name))
            && let Ok(dir) = self.open_child(&child, entry)
            && let Ok(iter) = dir.entries()
        {
            self.stack.push(Level::new(child, iter));
            self.open_dirs += 1;
            return;
        }
        if self.deferred < MAX_DEFERRED
            && let Some(level) = self.stack.last_mut()
        {
            level.deferred.push_back(name.to_os_string());
            self.deferred += 1;
        }
    }

    /// The innermost level is fully read and has released its handle: descend into the
    /// next subdirectory it postponed, or pop it once none are left. A postponed directory
    /// that still won't open is skipped — same best-effort contract as an unreadable one,
    /// and one retry keeps the walk finite.
    fn resume_deferred(&mut self) {
        let Some(level) = self.stack.last_mut() else {
            return;
        };
        let Some(name) = level.deferred.pop_front() else {
            self.stack.pop();
            return;
        };
        self.deferred -= 1;
        let path = level.path.join(name);
        // The level draining its deferrals released its own handle first, so `open_dirs`
        // is always below the cap here.
        if let Ok(dir) = self.open_path(&path)
            && let Ok(iter) = dir.entries()
        {
            self.stack.push(Level::new(path, iter));
            self.open_dirs += 1;
        }
    }

    /// The directory `entry` names at `path`, or the mounted volume when `path`
    /// is a mount point.
    fn open_child(&self, path: &Path, entry: &DirEntry) -> std::io::Result<Dir> {
        match self.mounts.iter().find(|(rel, _)| rel == path) {
            Some((_, volume)) => volume.try_clone(),
            None => entry.open_dir(),
        }
    }

    /// The directory at `path` below the base, through the volume mounted over
    /// it if any.
    fn open_path(&self, path: &Path) -> std::io::Result<Dir> {
        for (rel, volume) in &self.mounts {
            if let Ok(rest) = path.strip_prefix(rel) {
                if rest.as_os_str().is_empty() {
                    return volume.try_clone();
                }
                return volume.open_dir(rest);
            }
        }
        self.base.open_dir(path)
    }
}

pub fn kind_of(ft: &cap_std::fs::FileType) -> &'static str {
    if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    }
}

/// Directory walker (flat or recursive).
pub struct ChargedDirIter {
    walk: Option<ContainedWalk>,
    charge: ByteCharge,
}

impl ChargedDirIter {
    pub fn new(walk: ContainedWalk, limits: &TenantLimits) -> Result<Self, MemoryCapExceeded> {
        Ok(Self {
            walk: Some(walk),
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
        })
    }

    pub fn next_entry(&mut self) -> Option<WalkEntry> {
        self.walk.as_mut().and_then(ContainedWalk::next_entry)
    }

    pub fn close(&mut self) {
        self.walk = None;
        self.charge.release();
    }
}

/// Buffered writer to a temp sibling file. Explicit [`close`](Self::close) fsyncs and
/// atomically renames it onto `final_path`; `Drop` (the safety net for an un-`close`d
/// writer) discards the temp file without renaming, removing it only while its name
/// still refers to the writer's file.
pub struct ChargedFileWriter {
    /// `None` once the writer is closed.
    open: Option<OpenWrite>,
    /// Both sides stay contained paths for the whole lifetime of the writer. The commit
    /// happens through the same handle the open resolved against, so a link swapped over
    /// the parent between `writer()` and `close()` cannot redirect it — and the drop path
    /// cannot unlink something under the server's working directory.
    temp: TempFile,
    final_path: ContentPath,
    charge: ByteCharge,
}

/// The temp file a writer streams into. The program can reach it by name while the
/// writer is open, so before committing or discarding it, the writer checks that the
/// name still refers to the file it created.
pub struct TempFile {
    path: ContentPath,
    identity: FileIdentity,
}

impl TempFile {
    /// The temp file just created at `path` and opened as `file`.
    pub fn created(path: ContentPath, file: &cap_std::fs::File) -> std::io::Result<Self> {
        Ok(Self {
            path,
            identity: FileIdentity::of(&file.metadata()?)?,
        })
    }

    fn is_intact(&self) -> Result<bool, ContainError> {
        self.path.refers_to(self.identity)
    }
}

/// An open writer's buffer and its claim on the VFS's size limit. Program code runs
/// between writes, so the claim replaces nothing up front: every byte written counts
/// until `close` covers the file it replaces.
struct OpenWrite {
    writer: BufWriter<File>,
    disk_charge: QuotaCharge,
    /// Registers the temp file as the writer's own, so a removal of it by name
    /// leaves its bytes for the writer to settle.
    held: Option<OpenFileGuard>,
}

/// Why a write through a [`ChargedFileWriter`] failed.
#[derive(Debug)]
pub enum WriteError {
    Io(std::io::Error),
    Contain(ContainError),
    Full(QuotaExceeded),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Io(e) => e.fmt(f),
            WriteError::Contain(e) => e.fmt(f),
            WriteError::Full(e) => e.fmt(f),
        }
    }
}

impl ChargedFileWriter {
    pub fn new(
        file: File,
        temp: TempFile,
        final_path: ContentPath,
        limits: &TenantLimits,
        quota: Option<Arc<DiskQuota>>,
    ) -> Result<Self, MemoryCapExceeded> {
        let held = quota
            .as_ref()
            .map(|quota| quota.hold(temp.identity, Holder::Writer));
        Ok(Self {
            open: Some(OpenWrite {
                writer: BufWriter::new(file),
                disk_charge: QuotaCharge::new(quota, None),
                held,
            }),
            temp,
            final_path,
            charge: ByteCharge::new(limits, HANDLE_BUF_BYTES)?,
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), WriteError> {
        let open = self.reserve(line.len() as u64 + 1)?;
        writeln!(open.writer, "{line}").map_err(WriteError::Io)
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let open = self.reserve(bytes.len() as u64)?;
        open.writer.write_all(bytes).map_err(WriteError::Io)
    }

    fn reserve(&mut self, bytes: u64) -> Result<&mut OpenWrite, WriteError> {
        let open = self
            .open
            .as_mut()
            .ok_or_else(|| WriteError::Io(std::io::Error::other("writer already closed")))?;
        open.disk_charge.reserve(bytes).map_err(WriteError::Full)?;
        Ok(open)
    }

    /// Flush, fsync, and atomically rename temp→final. Idempotent: a second call after a
    /// successful close is a no-op.
    pub fn close(&mut self) -> Result<(), WriteError> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        let result = finalize(open, &self.temp, &self.final_path);
        self.charge.release();
        result
    }
}

impl Drop for ChargedFileWriter {
    fn drop(&mut self) {
        if let Some(open) = self.open.take() {
            // Discard the temp file rather than rename it: a GC-timed rename would be
            // non-deterministic.
            let unlinked = open
                .writer
                .into_inner()
                .ok()
                .is_some_and(|file| is_unlinked(&file));
            discard_temp(&self.temp, open.disk_charge, unlinked);
            drop(open.held);
        }
    }
}

/// Whether `file` has no name left, so its data goes once every handle on it
/// closes.
fn is_unlinked(file: &File) -> bool {
    cap_std::fs::Metadata::from_file(file)
        .is_ok_and(|meta| crate::runtime::fs::link_count(&meta) == Some(0))
}

/// Remove an uncommitted temp file and settle its disk charge. The program can reach
/// the temp file by name while the writer is open. If the name no longer refers to
/// the writer's file, it was moved or replaced: nothing is removed, and its bytes
/// stay counted while they may still be on disk under another name. Once the file
/// has no name at all, whether removed here or by the program, its bytes go when
/// the last handle on it closes.
fn discard_temp(temp: &TempFile, disk_charge: QuotaCharge, unlinked: bool) {
    let removed = temp.is_intact().unwrap_or(false) && temp.path.remove_file().is_ok();
    if removed || unlinked {
        disk_charge.release_when_closed(temp.identity);
    } else {
        disk_charge.keep();
    }
}

/// Commit the temp file over the final one. Any failure discards the temp file.
fn finalize(open: OpenWrite, temp: &TempFile, final_path: &ContentPath) -> Result<(), WriteError> {
    let OpenWrite {
        writer,
        mut disk_charge,
        held,
    } = open;
    let committed = match flush_and_sync(writer) {
        Ok(unlinked) => {
            let renamed = rename_into_place(temp, final_path, &mut disk_charge);
            match renamed {
                Ok(()) => disk_charge.commit(),
                Err(_) => discard_temp(temp, disk_charge, unlinked),
            }
            renamed
        }
        Err(err) => {
            discard_temp(temp, disk_charge, false);
            Err(err)
        }
    };
    drop(held);
    committed
}

/// Flush and sync the writer's file and close it, reporting whether the file has no
/// name left. Closing before the rename lets Windows handles release; unix `rename`
/// over an open file works either way.
fn flush_and_sync(writer: BufWriter<File>) -> Result<bool, WriteError> {
    let mut file = writer
        .into_inner()
        .map_err(|e| WriteError::Io(e.into_error()))?;
    file.flush().map_err(WriteError::Io)?;
    file.sync_all().map_err(WriteError::Io)?;
    Ok(is_unlinked(&file))
}

/// Rename the temp file over the final one, first covering what it replaces.
fn rename_into_place(
    temp: &TempFile,
    final_path: &ContentPath,
    disk_charge: &mut QuotaCharge,
) -> Result<(), WriteError> {
    if !temp.is_intact().map_err(WriteError::Contain)? {
        return Err(WriteError::Io(std::io::Error::other(
            "the writer's temporary file was moved or replaced",
        )));
    }
    disk_charge
        .cover(final_path.regular_file())
        .map_err(WriteError::Full)?;
    temp.path.rename_to(final_path).map_err(WriteError::Contain)
}

impl Closable for ChargedLineReader {
    fn close(&mut self) {
        ChargedLineReader::close(self);
    }
}

impl Closable for ChargedByteReader {
    fn close(&mut self) {
        ChargedByteReader::close(self);
    }
}

impl Closable for ChargedDirIter {
    fn close(&mut self) {
        ChargedDirIter::close(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file() -> File {
        tempfile::tempfile().expect("anonymous temp file")
    }

    #[test]
    fn drop_releases_bytes_back_to_limits() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let reader = ChargedLineReader::new(BufReader::new(temp_file()), &limits, None).unwrap();
        assert!(limits.host_attached_bytes() > 0, "new should charge bytes");
        drop(reader);
        assert_eq!(
            limits.host_attached_bytes(),
            0,
            "Drop must refund the charged bytes",
        );
    }

    #[test]
    fn drop_releases_one_of_many_proportionally() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let r1 = ChargedLineReader::new(BufReader::new(temp_file()), &limits, None).unwrap();
        let r2 = ChargedByteReader::new(temp_file(), 4096, &limits, None).unwrap();
        let total = limits.host_attached_bytes();
        assert_eq!(total, HANDLE_BUF_BYTES + (HANDLE_BUF_BYTES + 4096));
        drop(r1);
        assert_eq!(limits.host_attached_bytes(), HANDLE_BUF_BYTES + 4096);
        drop(r2);
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn close_refunds_eagerly_and_is_idempotent() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let mut reader =
            ChargedLineReader::new(BufReader::new(temp_file()), &limits, None).unwrap();
        assert!(limits.host_attached_bytes() > 0);
        reader.close();
        assert_eq!(limits.host_attached_bytes(), 0, "close refunds immediately");
        reader.close();
        drop(reader);
        assert_eq!(limits.host_attached_bytes(), 0, "no double refund");
    }

    /// A writer over a real VFS, since both of its paths are now contained.
    fn writer_over(
        vfs: &crate::runtime::Vfs,
        limits: &TenantLimits,
    ) -> (ChargedFileWriter, ContentPath, ContentPath) {
        use crate::runtime::fs::resolve_content;
        let final_path = resolve_content(vfs, "/", "/out.txt").unwrap();
        let temp_path = final_path.temp_sibling();
        let file = temp_path.create().unwrap();
        let temp = TempFile::created(temp_path.clone(), &file).unwrap();
        let w = ChargedFileWriter::new(file.into_std(), temp, final_path.clone(), limits, None)
            .unwrap();
        (w, temp_path, final_path)
    }

    fn temp_leftovers(root: &std::path::Path) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().starts_with("out.txt."))
            .collect()
    }

    #[test]
    fn unclosed_writer_drop_removes_temp_and_skips_rename() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let (mut w, _temp, _final) = writer_over(&vfs, &limits);
        w.write_line("hello").unwrap();
        assert!(limits.host_attached_bytes() > 0);
        drop(w);
        assert!(
            temp_leftovers(dir.path()).is_empty(),
            "temp file removed on drop"
        );
        assert!(
            !dir.path().join("out.txt").exists(),
            "no rename without explicit close"
        );
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    /// Builds `root/<branch>` as a chain `l1/l2/…/l{depth}` with an `f.txt` in every
    /// directory. Returns every path the walk should yield, relative to `root`.
    fn build_chain(root: &std::path::Path, branch: &str, depth: usize) -> Vec<String> {
        let mut path = root.join(branch);
        let mut rel = branch.to_string();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("f.txt"), "x").unwrap();
        let mut expected = vec![rel.clone(), format!("{rel}/f.txt")];
        for level in 1..=depth {
            path = path.join(format!("l{level}"));
            rel = format!("{rel}/l{level}");
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("f.txt"), "x").unwrap();
            expected.push(rel.clone());
            expected.push(format!("{rel}/f.txt"));
        }
        expected
    }

    fn walk_over(root: &std::path::Path, listed: &str, recursive: bool) -> ContainedWalk {
        let vfs = crate::runtime::Vfs::external(root.to_path_buf()).unwrap();
        let resolved = crate::runtime::fs::resolve_content(&vfs, "/", listed).unwrap();
        let base = Arc::new(resolved.open_dir().unwrap());
        let base_prefix = if listed == "/" {
            String::new()
        } else {
            format!("{}/", listed.trim_start_matches('/'))
        };
        ContainedWalk::new(base, base_prefix, recursive, Vec::new()).unwrap()
    }

    fn walk_all(root: &std::path::Path, listed: &str) -> Vec<WalkEntry> {
        let mut walk = walk_over(root, listed, true);
        std::iter::from_fn(|| walk.next_entry()).collect()
    }

    /// Every subtree is one consecutive run: for each directory, its descendants are
    /// yielded after it with nothing from outside the subtree interleaved.
    fn assert_subtrees_contiguous(entries: &[WalkEntry]) {
        for (i, dir) in entries.iter().enumerate() {
            if dir.kind != "directory" {
                continue;
            }
            let prefix = format!("{}/", dir.guest_path);
            let inside: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.guest_path.starts_with(&prefix))
                .map(|(j, _)| j)
                .collect();
            let (Some(&first), Some(&last)) = (inside.first(), inside.last()) else {
                continue;
            };
            assert!(
                first > i,
                "{} yielded before its parent",
                entries[first].guest_path
            );
            assert_eq!(
                last - first + 1,
                inside.len(),
                "entries from outside {} are interleaved into its subtree",
                dir.guest_path,
            );
        }
    }

    #[test]
    fn deep_tree_yields_every_entry_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut expected = build_chain(dir.path(), "a", MAX_OPEN_DIRS + 5);
        let mut got: Vec<String> = walk_all(dir.path(), "/")
            .into_iter()
            .map(|e| e.guest_path.trim_start_matches('/').to_string())
            .collect();
        got.sort();
        expected.sort();
        assert_eq!(
            got, expected,
            "deep walk must yield every entry exactly once"
        );
    }

    /// Past the open-handle cap the queue holds bare names, rejoined to the level's
    /// own path when the descent resumes. A wide level down there is where a mistake
    /// in that rejoin shows up — either as a doubled prefix or as a lost subtree.
    #[test]
    fn a_wide_level_past_the_open_dir_cap_walks_completely() {
        let dir = tempfile::tempdir().unwrap();
        let mut expected = build_chain(dir.path(), "a", MAX_OPEN_DIRS + 2);
        let deep = expected
            .iter()
            .filter(|p| !p.ends_with("f.txt"))
            .max_by_key(|p| p.matches('/').count())
            .expect("the deepest directory")
            .clone();
        for i in 0..64 {
            let child = format!("{deep}/w{i}");
            std::fs::create_dir(dir.path().join(&child)).unwrap();
            std::fs::write(dir.path().join(&child).join("f.txt"), "x").unwrap();
            expected.push(child.clone());
            expected.push(format!("{child}/f.txt"));
        }

        let mut got: Vec<String> = walk_all(dir.path(), "/")
            .into_iter()
            .map(|e| e.guest_path.trim_start_matches('/').to_string())
            .collect();
        got.sort();
        expected.sort();
        assert_eq!(
            got, expected,
            "every postponed subtree must still be walked"
        );
    }

    #[test]
    fn deep_tree_under_a_nested_base_keeps_the_base_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let expected: Vec<String> = build_chain(dir.path(), "a", MAX_OPEN_DIRS + 5)
            .into_iter()
            .filter(|p| p != "a")
            .map(|p| format!("/{p}"))
            .collect();
        let mut got: Vec<String> = walk_all(dir.path(), "/a")
            .into_iter()
            .map(|e| e.guest_path)
            .collect();
        let mut expected = expected;
        got.sort();
        expected.sort();
        assert_eq!(
            got, expected,
            "guest paths keep the listed directory's prefix"
        );
    }

    #[test]
    fn deep_subtrees_stay_contiguous_past_the_open_dir_cap() {
        let dir = tempfile::tempdir().unwrap();
        build_chain(dir.path(), "a", MAX_OPEN_DIRS + 5);
        build_chain(dir.path(), "b", MAX_OPEN_DIRS + 5);
        assert_subtrees_contiguous(&walk_all(dir.path(), "/"));
    }

    #[test]
    fn open_directory_handles_stay_within_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        build_chain(dir.path(), "a", MAX_OPEN_DIRS + 5);
        let mut walk = walk_over(dir.path(), "/", true);
        let mut deepest = 0;
        while walk.next_entry().is_some() {
            deepest = deepest.max(walk.open_dirs);
            assert!(
                walk.open_dirs <= MAX_OPEN_DIRS,
                "walk held {} directory handles",
                walk.open_dirs,
            );
        }
        assert_eq!(deepest, MAX_OPEN_DIRS, "a chain past the cap must reach it");
    }

    #[cfg(unix)]
    #[test]
    fn a_subdirectory_that_would_not_open_is_retried_from_the_base() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("w");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("inside.txt"), "x").unwrap();
        std::fs::write(dir.path().join("sibling.txt"), "x").unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&blocked).is_ok() {
            eprintln!("skipping: this user can read a 0o000 directory");
            return;
        }

        let mut walk = walk_over(dir.path(), "/", true);
        let mut paths = Vec::new();
        while let Some(entry) = walk.next_entry() {
            paths.push(entry.guest_path);
            // The descent into `/w` has already failed by the time it is yielded; the walk
            // must retry it when it drains, not drop the subtree.
            if entry.name == "w" {
                std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        assert!(
            paths.iter().any(|p| p == "/w/inside.txt"),
            "a subdirectory that failed to open must be retried, got {paths:?}",
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_deferred_directory_with_a_non_utf8_name_still_walks() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        build_chain(dir.path(), "a", MAX_OPEN_DIRS + 3);
        let mut deepest = dir.path().join("a");
        for level in 1..=(MAX_OPEN_DIRS + 3) {
            deepest = deepest.join(format!("l{level}"));
        }
        let odd = deepest.join(std::ffi::OsStr::from_bytes(b"bad\xffname"));
        // APFS (and any other filesystem enforcing UTF-8 names) rejects this outright,
        // which makes the case unreachable there rather than untested.
        if std::fs::create_dir(&odd).is_err() {
            eprintln!("skipping: this filesystem rejects non-UTF-8 names");
            return;
        }
        std::fs::write(odd.join("in.txt"), "x").unwrap();

        let entries = walk_all(dir.path(), "/");
        assert_eq!(
            entries.iter().filter(|e| e.name == "in.txt").count(),
            1,
            "a deferred directory whose name is not UTF-8 must still be reopened",
        );
    }

    #[test]
    fn writer_close_renames_and_refunds() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let vfs = crate::runtime::Vfs::external(dir.path().to_path_buf()).unwrap();
        let final_path = dir.path().join("out.txt");
        let (mut w, _temp, _final) = writer_over(&vfs, &limits);
        w.write_line("hello").unwrap();
        w.close().unwrap();
        assert!(final_path.exists());
        assert!(temp_leftovers(dir.path()).is_empty());
        assert_eq!(limits.host_attached_bytes(), 0);
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "hello\n");
    }
}
