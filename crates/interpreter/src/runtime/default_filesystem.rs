//! Default filesystem implementation over the contained VFS.
use super::durable::EffectId;
use super::filesystem::{
    DirectoryEntry, DirectoryReader, FileReader, FileWriter, Filesystem, FilesystemError,
    FilesystemInfo, MutationBudget, ReadItem, ReadOutput, ReadPosition, ReaderOptions,
    bounded_read,
};
use super::fs::{self, ContainError, ContentPath, FileIdentity, LinkPath, MAX_REMOVE_ENTRIES};
use super::fuel::WorkBudget;
use super::vfs::Placement;
use super::{DiskQuota, Holder, OpenFileGuard, QuotaCharge, QuotaExceeded, Vfs};
use crate::stdlib::shared::{contain_trap, quota_refusal, write_target_trap};
use cap_std::fs::{Dir, DirEntry, ReadDir};
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct DefaultFilesystem {
    vfs: Vfs,
}
impl DefaultFilesystem {
    pub fn new(vfs: Vfs) -> Self {
        Self { vfs }
    }
}
#[async_trait::async_trait]
impl Filesystem for DefaultFilesystem {
    fn info(&self) -> FilesystemInfo {
        FilesystemInfo::from_vfs(&self.vfs)
    }
    fn cwd(&self) -> &str {
        self.vfs.cwd()
    }
    fn placement(&self, path: &str) -> Result<Placement, FilesystemError> {
        Ok(fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?
            .placement()
            .clone())
    }
    async fn metadata(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, FilesystemError> {
        if follow {
            Ok(fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?.metadata()?)
        } else {
            Ok(fs::resolve_link(&self.vfs, self.vfs.cwd(), path)?.symlink_metadata()?)
        }
    }
    async fn read(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, FilesystemError> {
        let file = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?.open()?;
        bounded_read(file, offset, maximum_bytes)
    }
    async fn write(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        path.writable()?;
        if path.is_root() {
            return Err(FilesystemError::Unsupported(
                "cannot write to the VFS root or a mount point",
            ));
        }
        let mut quota = QuotaCharge::new(path.placement().quota().cloned(), path.regular_file());
        quota.reserve(bytes.len() as u64)?;
        let temp = path.temp_sibling();
        let mut file = temp.create_new()?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            temp.rename_to(&path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = temp.remove_file();
        } else {
            quota.commit();
        }
        result
    }
    async fn append(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        path.writable()?;
        if path.is_root() {
            return Err(FilesystemError::Unsupported(
                "cannot write to the VFS root or a mount point",
            ));
        }
        let count = bytes.len() as u64;
        let before = path.file_len();
        let mut quota = QuotaCharge::new(path.placement().quota().cloned(), None);
        quota.reserve(count)?;
        let result = path
            .append()
            .map_err(FilesystemError::from)
            .and_then(|mut file| file.write_all(&bytes).map_err(Into::into));
        if result.is_err() {
            quota
                .unreserve(count.saturating_sub(path.file_len().saturating_sub(before).min(count)));
            quota.keep();
        } else {
            quota.commit();
        }
        result
    }
    async fn mkdir(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
    ) -> Result<(), FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        if recursive {
            path.create_dir_all()?;
        } else {
            path.create_dir()?;
        }
        Ok(())
    }
    async fn remove(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        default_remove(&self.vfs, path, recursive, budget).map_err(FilesystemError::Execution)
    }
    async fn move_entry(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        from: &str,
        to: &str,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        default_move(&self.vfs, from, to, budget).map_err(FilesystemError::Execution)
    }
    async fn copy(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        from: &str,
        to: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        default_copy(&self.vfs, from, to, recursive, budget).map_err(FilesystemError::Execution)
    }
    async fn open_reader(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        options: ReaderOptions,
    ) -> Result<Box<dyn FileReader>, FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        let file = path.open()?;
        let held = path.placement().quota().and_then(|quota| {
            let metadata = file.metadata().ok()?;
            Some(quota.hold(fs::FileIdentity::of(&metadata).ok()?, super::Holder::Reader))
        });
        Ok(Box::new(DefaultReader {
            file: Some(file.into_std()),
            options,
            position: ReadPosition::default(),
            _held: held,
        }))
    }
    async fn open_directory(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        _: &super::limits::TenantLimits,
    ) -> Result<Box<dyn DirectoryReader>, FilesystemError> {
        let resolved = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        if !resolved.metadata()?.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).into());
        }
        let base = std::sync::Arc::new(resolved.open_dir()?);
        let mounts = if recursive {
            resolved.mounts_below()
        } else {
            Vec::new()
        };
        let normalized = fs::guest_normalize(self.vfs.cwd(), path).map_err(ContainError::from)?;
        let prefix = if normalized == "/" {
            String::new()
        } else {
            format!("{}/", normalized.trim_start_matches('/'))
        };
        let walk = ContainedWalk::new(base, prefix, recursive, mounts)?;
        Ok(Box::new(DefaultDirectory(Some(walk))))
    }
    async fn open_writer(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        path.writable()?;
        if path.is_root() {
            return Err(FilesystemError::Unsupported(
                "cannot write to the VFS root or a mount point",
            ));
        }
        let temp_path = path.temp_sibling();
        let file = temp_path.create_new()?;
        let temp = TempFile::created(temp_path, &file)?;
        let quota = path.placement().quota().cloned();
        Ok(Box::new(DefaultWriter::new(
            file.into_std(),
            temp,
            path,
            quota,
        )))
    }
}

struct DefaultReader {
    file: Option<std::fs::File>,
    options: ReaderOptions,
    position: ReadPosition,
    _held: Option<super::OpenFileGuard>,
}
#[async_trait::async_trait]
impl FileReader for DefaultReader {
    async fn next(
        &mut self,
        _effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        limits: &super::limits::TenantLimits,
    ) -> Result<ReadOutput, FilesystemError> {
        let Some(file) = &mut self.file else {
            return empty_read(limits);
        };
        read_item(
            file,
            &self.options,
            &mut self.position,
            work.as_deref(),
            limits,
        )
    }
    async fn close(
        &mut self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
    ) -> Result<(), FilesystemError> {
        self.file = None;
        self._held = None;
        self.position.closed = true;
        Ok(())
    }
}

/// Read one logical item, anchoring every attempt at the supplied position.
/// Read-ahead is local only. Errors leave the logical position unchanged.
/// Durable backends commit the returned position before exposing the result.
pub fn read_item(
    file: &mut (impl Read + Seek),
    options: &ReaderOptions,
    position: &mut ReadPosition,
    work: Option<&WorkBudget>,
    limits: &super::limits::TenantLimits,
) -> Result<ReadOutput, FilesystemError> {
    if position.closed {
        return empty_read(limits);
    }
    file.seek(SeekFrom::Start(position.offset))?;
    let mut next = position.clone();
    let result = match options {
        ReaderOptions::Bytes { chunk_size } => {
            read_chunk(file, *chunk_size, &mut next, work, limits)
        }
        ReaderOptions::Lines { maximum_bytes } => {
            read_line(file, *maximum_bytes, &mut next, work, limits)
        }
    }?;
    *position = next;
    Ok(result)
}
fn empty_read(limits: &super::limits::TenantLimits) -> Result<ReadOutput, FilesystemError> {
    Ok(ReadOutput {
        item: None,
        _charge: Some(memory(limits, 0)?),
    })
}
fn memory(
    limits: &super::limits::TenantLimits,
    bytes: u64,
) -> Result<super::limits::HostBytes, FilesystemError> {
    super::limits::HostBytes::new(limits, bytes)
        .map_err(|e| FilesystemError::Execution(wasmtime::Error::new(e)))
}
fn read_cost(
    work: Option<&WorkBudget>,
    rate: super::fuel::Rate,
    bytes: usize,
) -> Result<(), FilesystemError> {
    if let Some(work) = work {
        // The read has happened; exhaustion cannot discard its result.
        work.before_effect();
        work.charge(rate, bytes as u64)?;
    }
    Ok(())
}
fn read_chunk(
    file: &mut impl Read,
    size: usize,
    position: &mut ReadPosition,
    work: Option<&WorkBudget>,
    limits: &super::limits::TenantLimits,
) -> Result<ReadOutput, FilesystemError> {
    if size == 0 {
        return Err(FilesystemError::Rejected(
            "chunk size must be positive".into(),
        ));
    }
    if let Some(work) = work {
        work.charge(super::fuel::COPY, size as u64)?;
    }
    let charge = memory(limits, size as u64)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|e| FilesystemError::Backend(Box::new(e)))?;
    bytes.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        let count = file.read(&mut bytes[filled..])?;
        read_cost(work, super::fuel::IO, count)?;
        if count == 0 {
            break;
        }
        filled += count;
    }
    position.offset = position
        .offset
        .checked_add(filled as u64)
        .ok_or_else(|| FilesystemError::Rejected("reader offset overflow".into()))?;
    bytes.truncate(filled);
    Ok(ReadOutput {
        item: (filled > 0).then_some(ReadItem::Bytes(bytes)),
        _charge: Some(charge),
    })
}
fn read_line(
    file: &mut impl Read,
    maximum: u64,
    position: &mut ReadPosition,
    work: Option<&WorkBudget>,
    limits: &super::limits::TenantLimits,
) -> Result<ReadOutput, FilesystemError> {
    use std::io::BufRead;
    let mut charge = memory(limits, 8192)?;
    let mut reader = std::io::BufReader::new(file);
    let mut bytes = Vec::new();
    loop {
        let needs_read = reader.buffer().is_empty();
        let available = reader.fill_buf()?;
        if needs_read {
            read_cost(work, super::fuel::IO, available.len())?;
        }
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|&b| b == b'\n');
        let count = newline.map_or(available.len(), |i| i + 1);
        read_cost(work, super::fuel::SCAN, count)?;
        let length = bytes.len().checked_add(count).ok_or_else(|| {
            FilesystemError::Execution(super::host::range_error(
                "fs.lines: line length exceeds host capacity",
            ))
        })?;
        if length as u64 > maximum {
            return Err(FilesystemError::Execution(super::host::range_error(
                format!(
                    "fs.lines: line exceeds maxReadSize of {maximum} bytes; read smaller chunks with fs.bytes"
                ),
            )));
        }
        if length > bytes.capacity() {
            let capacity = length
                .max(bytes.capacity().saturating_mul(2))
                .min(usize::try_from(maximum).unwrap_or(usize::MAX));
            charge
                .grow(limits, (capacity - bytes.capacity()) as u64)
                .map_err(|e| FilesystemError::Execution(wasmtime::Error::new(e)))?;
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|e| FilesystemError::Backend(Box::new(e)))?;
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    if bytes.is_empty() {
        return empty_read(limits);
    }
    position.offset = position
        .offset
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| FilesystemError::Rejected("reader offset overflow".into()))?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    let capacity = bytes
        .len()
        .checked_mul(3)
        .ok_or_else(|| FilesystemError::Rejected("line capacity overflow".into()))?;
    let output_charge = memory(limits, capacity as u64)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|e| FilesystemError::Backend(Box::new(e)))?;
    for part in bytes.utf8_chunks() {
        text.push_str(part.valid());
        if !part.invalid().is_empty() {
            text.push('\u{fffd}');
        }
    }
    if !position.bom_handled {
        position.bom_handled = true;
        if text.starts_with('\u{feff}') {
            text.drain(..'\u{feff}'.len_utf8());
        }
    }
    Ok(ReadOutput {
        item: Some(ReadItem::Line(text)),
        _charge: Some(output_charge),
    })
}

struct DefaultDirectory(Option<ContainedWalk>);
#[async_trait::async_trait]
impl DirectoryReader for DefaultDirectory {
    async fn next_entry(
        &mut self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        _: &super::limits::TenantLimits,
    ) -> Result<Option<DirectoryEntry>, FilesystemError> {
        Ok(self.0.as_mut().and_then(ContainedWalk::next_entry))
    }
    fn retained_bytes(&self) -> u64 {
        8192
    }
    async fn close(
        &mut self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        _: &super::limits::TenantLimits,
    ) -> Result<(), FilesystemError> {
        self.0 = None;
        Ok(())
    }
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
    /// Lossy rendering of [`Level::path`] with guest `/` separators: `""` at the top,
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
        let mut prefix = String::new();
        for component in &path {
            prefix.push_str(&component.to_string_lossy());
            prefix.push('/');
        }
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
struct ContainedWalk {
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

    fn next_entry(&mut self) -> Option<DirectoryEntry> {
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
    fn visit(&mut self, entry: DirEntry) -> Option<DirectoryEntry> {
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
        Some(DirectoryEntry {
            name,
            path: guest_path,
            kind: kind_of(&ft),
            bytes: size,
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

/// Buffered writer to a temp sibling file. Explicit [`close`](Self::close) fsyncs and
/// atomically renames it onto `final_path`; `Drop` (the safety net for an un-`close`d
/// writer) discards the temp file without renaming, removing it only while its name
/// still refers to the writer's file.
struct DefaultWriter {
    /// `None` once the writer is closed.
    open: Option<OpenWrite>,
    /// Both sides stay contained paths for the whole lifetime of the writer. The commit
    /// happens through the same handle the open resolved against, so a link swapped over
    /// the parent between `writer()` and `close()` cannot redirect it — and the drop path
    /// cannot unlink something under the server's working directory.
    temp: TempFile,
    final_path: ContentPath,
}

/// The temp file a writer streams into. The program can reach it by name while the
/// writer is open, so before committing or discarding it, the writer checks that the
/// name still refers to the file it created.
struct TempFile {
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

/// Why a write through a [`DefaultWriter`] failed.
#[derive(Debug)]
enum WriteError {
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

impl DefaultWriter {
    pub fn new(
        file: File,
        temp: TempFile,
        final_path: ContentPath,
        quota: Option<Arc<DiskQuota>>,
    ) -> Self {
        let held = quota
            .as_ref()
            .map(|quota| quota.hold(temp.identity, Holder::Writer));
        Self {
            open: Some(OpenWrite {
                writer: BufWriter::new(file),
                disk_charge: QuotaCharge::new(quota, None),
                held,
            }),
            temp,
            final_path,
        }
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
        finalize(open, &self.temp, &self.final_path)
    }
}

impl Drop for DefaultWriter {
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

#[async_trait::async_trait]
impl FileWriter for DefaultWriter {
    fn is_closed(&self) -> bool {
        self.open.is_none()
    }
    async fn write(
        &mut self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        self.write_bytes(&bytes).map_err(Into::into)
    }
    async fn close(
        &mut self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
    ) -> Result<(), FilesystemError> {
        DefaultWriter::close(self).map_err(Into::into)
    }
}
impl From<WriteError> for FilesystemError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Io(e) => Self::Io(e),
            WriteError::Contain(e) => Self::Containment(e),
            WriteError::Full(e) => Self::Quota(e),
        }
    }
}

fn default_remove(
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
fn default_move(
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
fn default_copy(
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
struct CopyRun<'a> {
    recursive: bool,
    links: LinkCopies,
    /// `fs.copy` or `fs.move`, for diagnostics.
    op: &'a str,
    /// `from -> to` as the program wrote them, for diagnostics.
    pair: &'a str,
    /// The destination volume's limit.
    quota: Option<&'a Arc<DiskQuota>>,
}

/// What a copy does with a link inside the tree that the platform will not
/// reproduce, such as an absolute one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkCopies {
    /// Skip it: a copy leaves the source in place, so nothing is lost.
    BestEffort,
    /// Fail: a move removes the source afterwards, which would lose the link.
    Required,
}

/// Matches the entry-count limit used by removal.
const MAX_COPY_ENTRIES: u64 = MAX_REMOVE_ENTRIES as u64;
const MAX_COPY_DEPTH: usize = 64;

#[derive(Default)]
struct CopyWork {
    entries: u64,
    bytes: u64,
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

struct CopyFrame {
    from: LinkPath,
    to: LinkPath,
    from_dir: Arc<cap_std::fs::Dir>,
    to_dir: Arc<cap_std::fs::Dir>,
    entries: cap_std::fs::ReadDir,
}

/// Depth-first traversal retains directory iterators instead of recursing or
/// collecting every sibling path. Symlinks are copied as links, never followed.
fn copy_bounded(
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

fn copy_entry(
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

#[cfg(test)]
mod tests {
    #[test]
    fn logical_lines_bound_memory_and_restore_without_buffered_read_ahead() {
        use super::*;
        use crate::runtime::limits::TenantLimits;
        let limits = TenantLimits::new(1 << 20);
        let work = Arc::new(WorkBudget::new(u64::MAX));
        let mut file = std::io::Cursor::new(b"\xef\xbb\xbfhi\r\n\xff\n");
        let options = ReaderOptions::Lines { maximum_bytes: 32 };
        let mut position = ReadPosition::default();
        let output = read_item(
            &mut file,
            &options,
            &mut position,
            Some(work.as_ref()),
            &limits,
        )
        .unwrap();
        assert!(matches!(&output.item, Some(ReadItem::Line(line)) if line == "hi"));
        assert_eq!(position.offset, 7);
        assert!(limits.host_attached_bytes() > 0);
        drop(output);
        let output = read_item(
            &mut file,
            &options,
            &mut position,
            Some(work.as_ref()),
            &limits,
        )
        .unwrap();
        assert!(matches!(&output.item, Some(ReadItem::Line(line)) if line == "\u{fffd}"));
        drop(output);
        assert!(
            read_item(
                &mut file,
                &options,
                &mut position,
                Some(work.as_ref()),
                &limits
            )
            .unwrap()
            .item
            .is_none()
        );
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn line_limit_and_memory_refusal_leave_progress_retryable() {
        use super::*;
        use crate::runtime::limits::TenantLimits;
        let limits = TenantLimits::new(1 << 20);
        let mut position = ReadPosition::default();
        let mut file = std::io::Cursor::new(b"abcde");
        let options = ReaderOptions::Lines { maximum_bytes: 4 };
        for _ in 0..2 {
            let error = read_item(&mut file, &options, &mut position, None, &limits)
                .err()
                .unwrap();
            assert!(error.to_string().contains("maxReadSize"));
            assert_eq!(position.offset, 0);
            assert_eq!(limits.host_attached_bytes(), 0);
        }
        let limits = TenantLimits::new(8192 + 4);
        let mut file = std::io::Cursor::new(b"abcd");
        let error = read_item(&mut file, &options, &mut position, None, &limits)
            .err()
            .unwrap();
        assert!(
            matches!(error, FilesystemError::Execution(ref e) if e.is::<crate::runtime::limits::MemoryCapExceeded>())
        );
        assert_eq!(position.offset, 0);
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn read_failures_refund_buffers_and_charge_consumed_io_without_advancing_progress() {
        use super::*;
        use crate::runtime::limits::TenantLimits;
        struct FailsAfterOne(std::io::Cursor<[u8; 2]>);
        impl Read for FailsAfterOne {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if self.0.position() > 0 {
                    return Err(std::io::Error::other("injected read failure"));
                }
                self.0.read(&mut bytes[..1])
            }
        }
        impl Seek for FailsAfterOne {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.0.seek(position)
            }
        }
        let limits = TenantLimits::new(1 << 20);
        let work = Arc::new(crate::runtime::fuel::WorkBudget::new(u64::MAX));
        let mut file = FailsAfterOne(std::io::Cursor::new(*b"ab"));
        let mut position = ReadPosition::default();
        assert!(
            read_item(
                &mut file,
                &ReaderOptions::Bytes { chunk_size: 2 },
                &mut position,
                Some(work.as_ref()),
                &limits
            )
            .is_err()
        );
        assert_eq!(position.offset, 0);
        assert_eq!(limits.host_attached_bytes(), 0);
        assert!(work.spent() >= crate::runtime::fuel::IO.cost(1));
    }

    #[test]
    fn unknown_read_cost_settles_after_read_even_when_fuel_is_spent() {
        use super::*;
        let limits = crate::runtime::limits::TenantLimits::new(1 << 20);
        let work = Arc::new(crate::runtime::fuel::WorkBudget::new(0));
        let mut file = std::io::Cursor::new(b"line\n");
        let mut position = ReadPosition::default();
        assert!(
            read_item(
                &mut file,
                &ReaderOptions::Lines { maximum_bytes: 32 },
                &mut position,
                Some(work.as_ref()),
                &limits
            )
            .unwrap()
            .item
            .is_some()
        );
        assert_eq!(position.offset, 5);
        assert!(work.spent() > 0);
    }

    use super::*;
    use crate::runtime::fs::resolve_link;
    #[test]
    fn copy_entry_and_depth_limits_preserve_completed_work() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("source")).unwrap();
        std::fs::write(root.path().join("source/file"), "content").unwrap();
        let vfs = Vfs::external(root.path().to_path_buf()).unwrap();
        let from = resolve_link(&vfs, "/", "/source").unwrap();
        let to = resolve_link(&vfs, "/", "/target").unwrap();
        let run = CopyRun {
            recursive: true,
            links: LinkCopies::Required,
            op: "fs.copy",
            pair: "source -> target",
            quota: None,
        };
        let mut work = CopyWork {
            entries: MAX_COPY_ENTRIES - 1,
            bytes: 0,
        };
        let error = copy_bounded(&from, &to, &run, &mut work).unwrap_err();
        assert!(error.to_string().contains("entries"));
        assert!(root.path().join("target").is_dir());
        assert!(!root.path().join("target/file").exists());
        assert_eq!(
            std::fs::read(root.path().join("source/file")).unwrap(),
            b"content"
        );
        let to = resolve_link(&vfs, "/", "/too-deep").unwrap();
        let error = copy_entry(&from, &to, &run, &mut CopyWork::default(), MAX_COPY_DEPTH)
            .err()
            .unwrap();
        assert!(error.to_string().contains("directory levels"));
        assert!(!root.path().join("too-deep").exists());
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

    fn walk_all(root: &std::path::Path, listed: &str) -> Vec<DirectoryEntry> {
        let mut walk = walk_over(root, listed, true);
        std::iter::from_fn(|| walk.next_entry()).collect()
    }

    /// Every subtree is one consecutive run: for each directory, its descendants are
    /// yielded after it with nothing from outside the subtree interleaved.
    fn assert_subtrees_contiguous(entries: &[DirectoryEntry]) {
        for (i, dir) in entries.iter().enumerate() {
            if dir.kind != "directory" {
                continue;
            }
            let prefix = format!("{}/", dir.path);
            let inside: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.path.starts_with(&prefix))
                .map(|(j, _)| j)
                .collect();
            let (Some(&first), Some(&last)) = (inside.first(), inside.last()) else {
                continue;
            };
            assert!(
                first > i,
                "{} yielded before its parent",
                entries[first].path
            );
            assert_eq!(
                last - first + 1,
                inside.len(),
                "entries from outside {} are interleaved into its subtree",
                dir.path,
            );
        }
    }

    #[test]
    fn deep_tree_yields_every_entry_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut expected = build_chain(dir.path(), "a", MAX_OPEN_DIRS + 5);
        let mut got: Vec<String> = walk_all(dir.path(), "/")
            .into_iter()
            .map(|e| e.path.trim_start_matches('/').to_string())
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
            .map(|e| e.path.trim_start_matches('/').to_string())
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
            .map(|e| e.path)
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
            paths.push(entry.path);
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
}
