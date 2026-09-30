//! The blueprint's `size_limit`: a byte budget for the files in one VFS.
//!
//! Writers reserve before they write and release what they free, so the count
//! tracks the directory without walking it. Files a program holds open are
//! tracked too: an open file's data stays on disk after its name is removed, so
//! its bytes are freed when the last handle closes rather than when the name goes.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::runtime::fs::FileIdentity;

/// A byte budget for the files in one VFS.
#[derive(Debug)]
pub struct DiskQuota {
    limit: u64,
    used: AtomicU64,
    /// The directory couldn't be measured, so nothing that grows it fits.
    unmeasured: bool,
    /// Files a reader or writer holds open.
    open: Mutex<HashMap<FileIdentity, OpenFile>>,
}

#[derive(Debug, Default)]
struct OpenFile {
    readers: u32,
    writer: bool,
    /// Bytes whose name is gone, freed once no handle holds the file.
    pending: u64,
}

/// What holds a file open: a reader, or the writer that created it and settles
/// its bytes itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Reader,
    Writer,
}

impl DiskQuota {
    pub fn new(limit: u64, used: u64) -> Self {
        Self {
            limit,
            used: AtomicU64::new(used),
            unmeasured: false,
            open: Mutex::default(),
        }
    }

    /// A budget for a directory whose usage is unknown: every write that grows it
    /// is refused.
    pub fn unmeasured(limit: u64) -> Self {
        Self {
            unmeasured: true,
            ..Self::new(limit, 0)
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Acquire)
    }

    pub fn is_unmeasured(&self) -> bool {
        self.unmeasured
    }

    /// Claim `bytes`, or refuse without claiming anything when they don't fit.
    /// Claiming nothing always succeeds, so a write that doesn't grow the files
    /// works even in a directory already over its limit.
    pub fn reserve(&self, bytes: u64) -> Result<(), QuotaExceeded> {
        if bytes == 0 {
            return Ok(());
        }
        if self.unmeasured {
            return Err(QuotaExceeded::Unmeasured { limit: self.limit });
        }
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let Some(next) = used.checked_add(bytes).filter(|next| *next <= self.limit) else {
                return Err(QuotaExceeded::Over {
                    limit: self.limit,
                    used,
                    requested: bytes,
                });
            };
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok(()),
                Err(actual) => used = actual,
            }
        }
    }

    /// Count bytes already on disk, whether or not they fit: they are there.
    pub fn record(&self, bytes: u64) {
        let _ = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                Some(used.saturating_add(bytes))
            });
    }

    pub fn release(&self, bytes: u64) {
        let _ = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                Some(used.saturating_sub(bytes))
            });
    }

    /// Hold `file` open until the returned guard drops.
    pub fn hold(self: &Arc<Self>, file: FileIdentity, holder: Holder) -> OpenFileGuard {
        let mut open = self.open_files();
        let entry = open.entry(file).or_default();
        match holder {
            Holder::Reader => entry.readers = entry.readers.saturating_add(1),
            Holder::Writer => entry.writer = true,
        }
        OpenFileGuard {
            quota: Arc::clone(self),
            file,
            holder,
        }
    }

    /// Whether a reader or writer holds `file` open, so its bytes outlive its name.
    pub fn is_held(&self, file: FileIdentity) -> bool {
        self.open_files().contains_key(&file)
    }

    /// Whether `file` is a writer's own, still unsettled file.
    fn is_writers_own(&self, file: FileIdentity) -> bool {
        self.open_files().get(&file).is_some_and(|held| held.writer)
    }

    /// Every file a reader or writer holds open.
    pub fn held_files(&self) -> HashSet<FileIdentity> {
        self.open_files().keys().copied().collect()
    }

    /// A regular file's name was removed or replaced: free its `bytes` now, or
    /// when the last reader holding it closes. A writer's own file is left for
    /// the writer to settle.
    pub fn release_file(&self, file: FileIdentity, bytes: u64) {
        let mut open = self.open_files();
        match open.get_mut(&file) {
            Some(held) if held.writer => {}
            Some(held) => held.pending = held.pending.saturating_add(bytes),
            None => {
                drop(open);
                self.release(bytes);
            }
        }
    }

    /// A held file's name vanished in a change counted by measuring the
    /// directory, which already stopped counting its `bytes`: count them again
    /// until the handles holding it close. A writer's own file is the writer's
    /// to settle; a reader's is freed when the last reader closes.
    pub fn count_while_held(&self, file: FileIdentity, bytes: u64) {
        self.record(bytes);
        let mut open = self.open_files();
        if let Some(held) = open.get_mut(&file)
            && !held.writer
        {
            held.pending = held.pending.saturating_add(bytes);
        }
    }

    /// Settle the bytes a writer wrote to its own file once the file has no name
    /// left. Called by the writer while it still holds `file`, so only readers can
    /// keep the file on disk: free the bytes now, or when the last reader closes.
    fn release_writers_own_file(&self, file: FileIdentity, bytes: u64) {
        let mut open = self.open_files();
        match open.get_mut(&file) {
            Some(held) if held.readers > 0 => held.pending = held.pending.saturating_add(bytes),
            _ => {
                drop(open);
                self.release(bytes);
            }
        }
    }

    fn open_files(&self) -> MutexGuard<'_, HashMap<FileIdentity, OpenFile>> {
        // A poisoned lock still holds consistent counters: every update is one
        // assignment, so recovering it loses nothing.
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Keeps a file registered as open with a [`DiskQuota`] while a handle holds it.
#[derive(Debug)]
pub struct OpenFileGuard {
    quota: Arc<DiskQuota>,
    file: FileIdentity,
    holder: Holder,
}

impl Drop for OpenFileGuard {
    fn drop(&mut self) {
        let mut open = self.quota.open_files();
        let Some(held) = open.get_mut(&self.file) else {
            return;
        };
        match self.holder {
            Holder::Reader => held.readers = held.readers.saturating_sub(1),
            Holder::Writer => held.writer = false,
        }
        if held.readers == 0 && !held.writer {
            let pending = held.pending;
            open.remove(&self.file);
            drop(open);
            self.quota.release(pending);
        }
    }
}

/// One write's claim on a [`DiskQuota`], from its first byte until it commits or
/// is abandoned.
///
/// A write that replaces a file within one host call, where no program code can
/// touch that file before it commits, may draw on the file's size first and
/// reserve only the growth beyond it: its `allowance`. Only a file no handle holds
/// gives an allowance, since a held file's bytes stay on disk after its name
/// goes. A write the program can interleave with replaces nothing up front, so
/// every byte it writes counts until it commits, and re-reads what it replaces
/// with [`cover`](Self::cover) just before committing. [`commit`](Self::commit)
/// then frees the replaced file. A write that truncates and rewrites a file itself
/// ([`in_place`](Self::in_place)) draws on the old contents even while readers
/// hold the file, since they are gone either way; a writer's own file is left for
/// the writer to settle. Dropping an uncommitted charge gives back
/// everything it reserved, for a write whose output is gone.
#[derive(Debug)]
pub struct QuotaCharge {
    quota: Option<Arc<DiskQuota>>,
    allowance: u64,
    /// Allowance used so far; never more than `allowance`.
    drawn: u64,
    reserved: u64,
    replaces: Replaces,
}

/// What a committed write frees.
#[derive(Debug, Clone, Copy)]
enum Replaces {
    /// Nothing: the write's name reached no regular file, or reached a writer's
    /// own file, which the writer settles.
    Nothing,
    /// The regular file the write's new file takes the name of, and its size: the
    /// allowance's file until `cover` re-reads it.
    File(FileIdentity, u64),
    /// The old contents of the file the write rewrites itself: its allowance.
    InPlace,
}

impl Replaces {
    fn file(replaced: Option<(FileIdentity, u64)>) -> Self {
        replaced.map_or(Self::Nothing, |(file, bytes)| Self::File(file, bytes))
    }
}

impl QuotaCharge {
    /// A charge that may draw on `replaced`, the regular file the write replaces.
    pub fn new(quota: Option<Arc<DiskQuota>>, replaced: Option<(FileIdentity, u64)>) -> Self {
        let allowance = match (&quota, replaced) {
            (Some(quota), Some((file, bytes))) if !quota.is_held(file) => bytes,
            _ => 0,
        };
        Self {
            quota,
            allowance,
            drawn: 0,
            reserved: 0,
            replaces: Replaces::file(replaced),
        }
    }

    /// A charge for rewriting `rewritten`, a regular file and its size, in place:
    /// the same file takes the new contents, so the old ones are freed at once even
    /// while readers hold it. A writer's own file is the writer's to settle, so
    /// rewriting it frees nothing and every new byte counts.
    pub fn in_place(quota: Option<Arc<DiskQuota>>, rewritten: Option<(FileIdentity, u64)>) -> Self {
        let (allowance, replaces) = match (&quota, rewritten) {
            (Some(quota), Some((file, bytes))) if !quota.is_writers_own(file) => {
                (bytes, Replaces::InPlace)
            }
            _ => (0, Replaces::Nothing),
        };
        Self {
            quota,
            allowance,
            drawn: 0,
            reserved: 0,
            replaces,
        }
    }

    /// Claim `bytes` about to be written: from the allowance first, the rest from
    /// the quota.
    pub fn reserve(&mut self, bytes: u64) -> Result<(), QuotaExceeded> {
        let from_allowance = bytes.min(self.allowance.saturating_sub(self.drawn));
        let beyond = bytes.saturating_sub(from_allowance);
        if let Some(quota) = &self.quota {
            quota.reserve(beyond)?;
        }
        self.drawn = self.drawn.saturating_add(from_allowance);
        self.reserved = self.reserved.saturating_add(beyond);
        Ok(())
    }

    /// Give back bytes claimed by the last [`reserve`](Self::reserve) that were
    /// never written, as after a short write.
    pub fn unreserve(&mut self, bytes: u64) {
        let from_reserved = bytes.min(self.reserved);
        if let Some(quota) = &self.quota {
            quota.release(from_reserved);
        }
        self.reserved = self.reserved.saturating_sub(from_reserved);
        self.drawn = self
            .drawn
            .saturating_sub(bytes.saturating_sub(from_reserved));
    }

    /// Settle against the file the write is about to replace, as it stands now:
    /// whatever the write drew from its allowance beyond that file's size must be
    /// reserved after all, or the write refused.
    pub fn cover(&mut self, replaced: Option<(FileIdentity, u64)>) -> Result<(), QuotaExceeded> {
        let replaced_bytes = replaced.map_or(0, |(_, bytes)| bytes);
        let uncovered = self.drawn.saturating_sub(replaced_bytes);
        if let Some(quota) = &self.quota {
            quota.reserve(uncovered)?;
        }
        self.drawn = self.drawn.saturating_sub(uncovered);
        self.reserved = self.reserved.saturating_add(uncovered);
        self.replaces = Replaces::file(replaced);
        Ok(())
    }

    /// The write landed: free what it replaced beyond the allowance it drew on.
    /// A replaced file goes once no handle holds it; old contents rewritten in
    /// place go at once.
    pub fn commit(mut self) {
        let Some(quota) = self.quota.take() else {
            return;
        };
        match self.replaces {
            Replaces::Nothing => {}
            Replaces::File(file, bytes) => {
                quota.release_file(file, bytes.saturating_sub(self.drawn));
            }
            Replaces::InPlace => quota.release(self.allowance.saturating_sub(self.drawn)),
        }
    }

    /// The write failed partway, leaving its file at `left` bytes: count the file
    /// at that size. A file rewritten in place had its old contents counted too;
    /// otherwise no more than was reserved can be the write's own.
    pub fn settle_at(mut self, left: u64) {
        let Some(quota) = self.quota.take() else {
            return;
        };
        match self.replaces {
            Replaces::InPlace => {
                let counted = self.allowance.saturating_add(self.reserved);
                if left < counted {
                    quota.release(counted - left);
                } else {
                    quota.record(left - counted);
                }
            }
            Replaces::Nothing | Replaces::File(..) => {
                quota.release(self.reserved.saturating_sub(left));
            }
        }
    }

    /// The write failed but its bytes may still be on disk, under a name the
    /// program gave them: keep them counted rather than giving them back.
    pub fn keep(mut self) {
        self.quota = None;
    }

    /// The write failed and its file, `written`, has no name left: its bytes go
    /// once every handle on it closes.
    pub fn release_when_closed(mut self, written: FileIdentity) {
        if let Some(quota) = self.quota.take() {
            quota.release_writers_own_file(written, self.reserved);
        }
    }
}

impl Drop for QuotaCharge {
    fn drop(&mut self) {
        if let Some(quota) = self.quota.take() {
            quota.release(self.reserved);
        }
    }
}

/// A write refused by the VFS's size limit: over it, or unable to tell because
/// the directory couldn't be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaExceeded {
    /// The write would take the files past the limit.
    Over {
        limit: u64,
        used: u64,
        requested: u64,
    },
    /// The directory couldn't be measured when the run started, so it is
    /// treated as full.
    Unmeasured { limit: u64 },
}

impl fmt::Display for QuotaExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Over {
                limit,
                used,
                requested,
            } => write!(
                f,
                "the filesystem's size limit of {limit} bytes would be exceeded: {used} bytes are in use and this needs {requested} more"
            ),
            Self::Unmeasured { limit } => write!(
                f,
                "the filesystem couldn't be measured against its size limit of {limit} bytes, so it is treated as full until a run can measure it"
            ),
        }
    }
}

impl std::error::Error for QuotaExceeded {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_reserves_within_the_limit_and_refuses_past_it() {
        let quota = DiskQuota::new(100, 40);
        quota.reserve(60).expect("fits exactly");
        let refused = quota.reserve(1).expect_err("one byte over");
        assert_eq!(
            refused,
            QuotaExceeded::Over {
                limit: 100,
                used: 100,
                requested: 1,
            }
        );
        quota.release(30);
        assert_eq!(quota.used(), 70);
        quota.release(1_000);
        assert_eq!(quota.used(), 0, "release never underflows");
    }

    #[test]
    fn a_dropped_charge_gives_back_what_it_reserved() {
        let quota = Arc::new(DiskQuota::new(100, 0));
        let mut charge = QuotaCharge::new(Some(Arc::clone(&quota)), None);
        charge.reserve(70).expect("fits");
        assert_eq!(quota.used(), 70);
        drop(charge);
        assert_eq!(quota.used(), 0);
    }

    #[test]
    fn a_charge_covers_a_replaced_file_that_shrank() {
        let quota = Arc::new(DiskQuota::new(100, 90));
        let file = identity();
        let mut charge = QuotaCharge::new(Some(Arc::clone(&quota)), Some((file, 90)));
        charge.reserve(90).expect("drawn from the allowance");
        assert_eq!(quota.used(), 90);
        // Something empties the file before the write commits, freeing its bytes;
        // the 90 the write drew on them must now be reserved after all.
        quota.release(90);
        charge
            .cover(Some((file, 0)))
            .expect("fits once the file is empty");
        charge.commit();
        assert_eq!(quota.used(), 90);
        let mut again = QuotaCharge::new(Some(Arc::clone(&quota)), Some((file, 90)));
        again.reserve(90).expect("drawn from the allowance");
        assert!(
            again.cover(Some((file, 0))).is_err(),
            "the file shrank and nothing freed its bytes"
        );
    }

    #[test]
    fn a_held_file_gives_no_allowance() {
        let quota = Arc::new(DiskQuota::new(100, 60));
        let file = identity();
        let reader = quota.hold(file, Holder::Reader);
        let mut charge = QuotaCharge::new(Some(Arc::clone(&quota)), Some((file, 60)));
        assert!(
            charge.reserve(60).is_err(),
            "the replaced file stays on disk while the reader holds it"
        );
        charge.reserve(40).expect("the growth beyond it fits");
        charge.commit();
        assert_eq!(quota.used(), 100, "the old file is still counted");
        drop(reader);
        assert_eq!(quota.used(), 40, "and freed when the reader closes");
    }

    #[test]
    fn a_file_rewritten_in_place_frees_its_old_bytes_while_held() {
        let quota = Arc::new(DiskQuota::new(100, 60));
        let file = identity();
        let _reader = quota.hold(file, Holder::Reader);
        let mut charge = QuotaCharge::in_place(Some(Arc::clone(&quota)), Some((file, 60)));
        charge.reserve(30).expect("drawn from the old contents");
        charge.commit();
        assert_eq!(quota.used(), 30, "the old contents are gone, held or not");
    }

    #[test]
    fn a_failed_rewrite_in_place_counts_what_it_left() {
        let quota = Arc::new(DiskQuota::new(100, 40));
        let mut charge = QuotaCharge::in_place(Some(Arc::clone(&quota)), Some((identity(), 40)));
        charge.reserve(50).expect("room for the new contents");
        assert_eq!(quota.used(), 50);
        charge.settle_at(20);
        assert_eq!(
            quota.used(),
            20,
            "the old contents are gone, 20 new bytes landed"
        );

        let untouched = Arc::new(DiskQuota::new(100, 40));
        let mut charge =
            QuotaCharge::in_place(Some(Arc::clone(&untouched)), Some((identity(), 40)));
        charge.reserve(10).expect("drawn from the old contents");
        charge.settle_at(40);
        assert_eq!(
            untouched.used(),
            40,
            "a copy that failed before truncating left the file"
        );
    }

    #[test]
    fn a_writers_own_file_rewritten_in_place_is_left_for_the_writer() {
        let quota = Arc::new(DiskQuota::new(100, 60));
        let file = identity();
        let _writer = quota.hold(file, Holder::Writer);
        let mut charge = QuotaCharge::in_place(Some(Arc::clone(&quota)), Some((file, 60)));
        charge.reserve(30).expect("room for the new contents");
        charge.commit();
        assert_eq!(
            quota.used(),
            90,
            "the writer's charge still holds the old contents"
        );
    }

    #[test]
    fn a_removed_file_held_open_stays_counted_until_it_closes() {
        let quota = Arc::new(DiskQuota::new(100, 60));
        let file = identity();
        let reader = quota.hold(file, Holder::Reader);
        quota.release_file(file, 60);
        assert_eq!(quota.used(), 60, "an open file's bytes are still on disk");
        drop(reader);
        assert_eq!(quota.used(), 0);
    }

    #[test]
    fn a_writers_own_file_is_left_for_the_writer_to_settle() {
        let quota = Arc::new(DiskQuota::new(100, 0));
        let file = identity();
        let writer = quota.hold(file, Holder::Writer);
        quota.record(40);
        quota.release_file(file, 40);
        assert_eq!(quota.used(), 40);
        drop(writer);
        assert_eq!(
            quota.used(),
            40,
            "releasing it is the writer's charge's job"
        );
    }

    #[test]
    fn a_held_file_that_vanished_is_counted_until_its_reader_closes() {
        let quota = Arc::new(DiskQuota::new(100, 0));
        let file = identity();
        let reader = quota.hold(file, Holder::Reader);
        quota.count_while_held(file, 30);
        assert_eq!(quota.used(), 30);
        drop(reader);
        assert_eq!(quota.used(), 0);
    }

    /// The identity of a fresh file, for tests that need one.
    fn identity() -> FileIdentity {
        let file = tempfile::tempfile().expect("tempfile");
        let metadata = cap_std::fs::Metadata::from_file(&file).expect("metadata");
        FileIdentity::of(&metadata).expect("identity")
    }
}
