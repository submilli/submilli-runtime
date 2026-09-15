//! Durable ledger backing `Idempotency-Key` on the session execute endpoint.
//!
//! One entry per `(session_id, key)`, laid out as
//! `<root>/<hex(session_id)>/<hex(key)>.json`. Both components are
//! caller-controlled and may contain path separators, so both are hex-encoded —
//! the same defence [`crate::session_store`] applies to the session id alone. A
//! directory per session (rather than one growing record) keeps each write flat
//! and makes cleanup a single recursive remove.
//!
//! Two things this store deliberately does differently from
//! [`crate::session_store`], which it otherwise mirrors:
//!
//! * **Errors propagate.** `SessionManager::persist` logs and swallows because
//!   its in-memory cache is authoritative and the file is a mirror. Here the
//!   durable entry *is* the authority: a reservation the server cannot record
//!   must refuse the request rather than execute unguarded.
//! * **Filesystem work runs on the blocking pool.** The session store gets away
//!   with blocking `std::fs` inside an `async fn` because its writes are
//!   debounced 30s apart; a ledger write happens twice per keyed execute and
//!   fsyncs, which would park a tokio worker thread on disk latency.
//!
//! On disk an entry is only ever `reserved` or `completed`. *Indeterminate* is
//! derived, never written: a `reserved` entry with no live in-flight handle —
//! after a crash, or after a guard dropped without an outcome — is indeterminate
//! by definition.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::blueprint::StoreError;

/// One ledger entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub session_id: String,
    pub key: String,
    /// SHA-256 of the request code. A key reused with a different fingerprint is
    /// a conflict, never a replay.
    pub fingerprint: String,
    pub state: EntryState,
    /// When the key was first reserved, carried unchanged through completion.
    ///
    /// Nothing reads it yet. It is recorded now because entries are only
    /// reclaimed when their whole session goes, so a long-lived session
    /// accumulates them without bound, and every eviction policy worth having
    /// — oldest-first, TTL — needs an age to order by. File `mtime` cannot
    /// stand in: a backup, a `cp`, or an rsync of the store loses it. Adding
    /// the field later would mean either a migration or tolerating its absence
    /// forever, so it costs least here.
    pub created_at: SystemTime,
}

/// What the ledger knows about a key. *Indeterminate* is deliberately absent:
/// it is not a state an entry is written in, it is what [`EntryState::Reserved`]
/// means once no live handle owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// The key is claimed and the program may be running. Nothing to replay.
    Reserved,
    /// The program finished and this is the response to replay.
    Completed(RecordedOutcome),
}

/// The response a completed entry replays: the exact status and body bytes the
/// original caller received. Stored verbatim rather than as a re-serialized
/// struct so a replay is byte-identical.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedOutcome {
    pub status: u16,
    pub body: String,
}

impl LedgerEntry {
    pub fn reserved(session_id: &str, key: &str, fingerprint: String) -> Self {
        Self {
            session_id: session_id.to_string(),
            key: key.to_string(),
            fingerprint,
            state: EntryState::Reserved,
            created_at: now_to_stored_precision(),
        }
    }

    /// The recorded response, if this key has one to replay.
    pub fn outcome(&self) -> Option<&RecordedOutcome> {
        match &self.state {
            EntryState::Reserved => None,
            EntryState::Completed(outcome) => Some(outcome),
        }
    }
}

/// Now, truncated to the millisecond the on-disk form stores. Taking the
/// timestamp at the precision it will be persisted at makes a write followed by
/// a read an identity; keeping the nanoseconds would mean a loaded entry never
/// compares equal to the one that was written.
fn now_to_stored_precision() -> SystemTime {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH + Duration::from_millis(saturating_millis(since_epoch))
}

fn saturating_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The fingerprint a key is bound to. Covers the code and nothing else — a
/// replay executes nothing, so a blueprint tightened since the original run is
/// not bypassed by re-delivering bytes that session already received.
pub fn code_fingerprint(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Async to mirror [`DurableSessionStore`](crate::session_store::DurableSessionStore)
/// so a DB-backed implementation can be swapped in without reshaping call sites.
#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// Insert or replace the entry for `(entry.session_id, entry.key)`.
    async fn put(&self, entry: LedgerEntry) -> Result<(), StoreError>;
    async fn load(&self, session_id: &str, key: &str) -> Result<Option<LedgerEntry>, StoreError>;
    /// Drop one entry. Used when a reservation is released without an outcome
    /// because nothing was ever dispatched — a retry with that key must
    /// find no entry and run.
    async fn remove(&self, session_id: &str, key: &str) -> Result<(), StoreError>;
    /// Drop every entry for a session, whatever state each is in.
    async fn purge_session(&self, session_id: &str) -> Result<(), StoreError>;
    /// Every session id holding at least one entry. Boot reconciliation reads
    /// this to purge ledgers whose session did not come back.
    async fn session_ids(&self) -> Vec<String>;
}

/// Ephemeral store for tests and the no-persistence fallback.
#[derive(Default)]
pub struct InMemoryIdempotencyStore {
    inner: Mutex<HashMap<String, HashMap<String, LedgerEntry>>>,
}

#[async_trait::async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn put(&self, entry: LedgerEntry) -> Result<(), StoreError> {
        self.lock()
            .entry(entry.session_id.clone())
            .or_default()
            .insert(entry.key.clone(), entry);
        Ok(())
    }

    async fn load(&self, session_id: &str, key: &str) -> Result<Option<LedgerEntry>, StoreError> {
        Ok(self
            .lock()
            .get(session_id)
            .and_then(|entries| entries.get(key))
            .cloned())
    }

    async fn remove(&self, session_id: &str, key: &str) -> Result<(), StoreError> {
        if let Some(entries) = self.lock().get_mut(session_id) {
            entries.remove(key);
        }
        Ok(())
    }

    async fn purge_session(&self, session_id: &str) -> Result<(), StoreError> {
        self.lock().remove(session_id);
        Ok(())
    }

    async fn session_ids(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }
}

impl InMemoryIdempotencyStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, LedgerEntry>>> {
        self.inner.lock().expect("idempotency store mutex poisoned")
    }
}

/// Crash-safe, file-backed ledger. Entries are owner-only on disk: they hold
/// program output, including the console capture a failing run returns.
pub struct FileIdempotencyStore {
    root: PathBuf,
    /// One cell per session directory, resolved the first time anything writes
    /// into that session and holding "the directory exists *and* its entry in
    /// the store root is durable".
    ///
    /// A directory entry only becomes durable once its parent is synced, so
    /// every writer needs that guarantee — not just whichever one happened to
    /// create the directory. A plain "have we synced this yet" flag cannot
    /// provide it: the second writer would find the flag set and proceed while
    /// the first is still between `mkdir` and its own fsync. `OnceCell` makes
    /// the others *wait* on the initializer instead of skipping past it, and
    /// caches nothing on failure, so a write that fails mid-sync leaves the
    /// next one to redo it rather than inheriting a durability claim that was
    /// never earned.
    session_dirs: Mutex<HashMap<String, Arc<OnceCell<()>>>>,
}

impl FileIdempotencyStore {
    /// Open the store, creating `root` if absent.
    pub fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        restrict_dir(&root)?;
        // The root's own directory entry needs its parent synced for the same
        // reason its children do — otherwise a crash can take the whole store
        // with it, and the first reservations vanish. `create_dir_all` may have
        // created several levels, so sync from the outermost one that exists
        // down to the root itself.
        sync_ancestry(&root)?;
        Ok(Self {
            root,
            session_dirs: Mutex::new(HashMap::new()),
        })
    }

    /// The cell guarding this session's directory. Cloned out from under the
    /// lock so the initializer below never runs while the map is held.
    fn session_dir_cell(&self, dir_name: &str) -> Arc<OnceCell<()>> {
        Arc::clone(
            self.session_dirs
                .lock()
                .expect("idempotency session dir map poisoned")
                .entry(dir_name.to_string())
                .or_default(),
        )
    }

    /// Create the session directory and make its entry in the store root
    /// durable, once per session per process. Concurrent callers await the
    /// first; an error is not cached, so the next caller retries.
    async fn ensure_session_dir(&self, dir_name: &str) -> Result<(), StoreError> {
        let cell = self.session_dir_cell(dir_name);
        let root = self.root.clone();
        let session_dir = root.join(dir_name);
        cell.get_or_try_init(|| {
            blocking(move || {
                create_session_dir(&session_dir)?;
                File::open(&root)?.sync_all()
            })
        })
        .await
        .map(|_| ())
    }

    /// Forget this session's cell so a directory recreated after a purge earns
    /// its root fsync again — and so the map stays bounded by live sessions
    /// rather than by every session the process has ever seen.
    fn forget_session_dir(&self, dir_name: &str) {
        self.session_dirs
            .lock()
            .expect("idempotency session dir map poisoned")
            .remove(dir_name);
    }
}

#[async_trait::async_trait]
impl IdempotencyStore for FileIdempotencyStore {
    async fn put(&self, entry: LedgerEntry) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(&StoredEntry::from_entry(&entry))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let dir_name = hex_encode(&entry.session_id);
        self.ensure_session_dir(&dir_name).await?;
        let session_dir = self.root.join(&dir_name);
        let file_name = entry_file_name(&entry.key);
        blocking(move || write_entry(&session_dir, &file_name, &bytes)).await
    }

    async fn load(&self, session_id: &str, key: &str) -> Result<Option<LedgerEntry>, StoreError> {
        let path = self
            .root
            .join(hex_encode(session_id))
            .join(entry_file_name(key));
        let bytes = blocking(move || match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        })
        .await?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        // A corrupt entry is an error, not an absence. Reporting it absent would
        // let the request execute a second time — the exact outcome the ledger
        // exists to prevent.
        serde_json::from_slice::<StoredEntry>(&bytes)
            .map(|stored| Some(stored.into_entry()))
            .map_err(|e| StoreError::Io(format!("malformed idempotency entry: {e}")))
    }

    async fn remove(&self, session_id: &str, key: &str) -> Result<(), StoreError> {
        let session_dir = self.root.join(hex_encode(session_id));
        let path = session_dir.join(entry_file_name(key));
        blocking(move || match fs::remove_file(&path) {
            Ok(()) => File::open(&session_dir)?.sync_all(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        })
        .await
    }

    async fn purge_session(&self, session_id: &str) -> Result<(), StoreError> {
        let root = self.root.clone();
        let dir_name = hex_encode(session_id);
        // Drop the cell before the removal, not after: a write that races this
        // purge must re-create *and* re-sync rather than trusting a durability
        // claim that the removal is about to invalidate.
        self.forget_session_dir(&dir_name);
        let session_dir = root.join(&dir_name);
        blocking(move || match fs::remove_dir_all(&session_dir) {
            Ok(()) => File::open(&root)?.sync_all(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        })
        .await
    }

    async fn session_ids(&self) -> Vec<String> {
        let root = self.root.clone();
        let names = blocking(move || {
            let mut names = Vec::new();
            match fs::read_dir(&root) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if entry.path().is_dir()
                            && let Some(name) = entry.file_name().to_str()
                        {
                            names.push(name.to_string());
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            Ok(names)
        })
        .await;
        match names {
            Ok(names) => names.iter().filter_map(|name| hex_decode(name)).collect(),
            Err(err) => {
                tracing::warn!(?err, "cannot read idempotency store dir");
                Vec::new()
            }
        }
    }
}

/// Run blocking filesystem work off the async worker threads. See the module
/// docs for why this store does not follow `session_store`'s inline I/O.
async fn blocking<T, F>(work: F) -> Result<T, StoreError>
where
    F: FnOnce() -> io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(StoreError::Io(e.to_string())),
        Err(e) => Err(StoreError::Io(format!(
            "idempotency store task failed: {e}"
        ))),
    }
}

/// Write one entry durably: temp → fsync → atomic rename → fsync the session
/// directory.
///
/// The session directory is assumed to exist and to be durable already —
/// `ensure_session_dir` owns that half, and owns it exactly once per session so
/// that concurrent writers cannot proceed on a directory entry whose own fsync
/// is still pending. A write that finds the directory gone (a purge landed
/// first) fails rather than recreating it: the session is over, and an entry
/// recorded against it could never be read back.
fn write_entry(session_dir: &Path, file_name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = session_dir.join(format!(".{file_name}.tmp"));
    let mut file = create_owner_only(&tmp)?;
    file.write_all(bytes)?;
    restrict_file(&tmp)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, session_dir.join(file_name))?;
    File::open(session_dir)?.sync_all()
}

/// Make `dir`'s own directory entry durable, and every ancestor's along with
/// it. `create_dir_all` can create several levels at once, so syncing only the
/// immediate parent would leave the levels above it undurable — and a crash
/// would then take the whole store, not just the newest entries.
fn sync_ancestry(dir: &Path) -> io::Result<()> {
    let mut current = dir.parent();
    while let Some(parent) = current {
        // An ancestor that is not there to open is one we did not create.
        match File::open(parent) {
            Ok(handle) => handle.sync_all()?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        }
        current = parent.parent();
    }
    Ok(())
}

/// Create the session directory if absent, at owner-only permissions.
fn create_session_dir(dir: &Path) -> io::Result<()> {
    match fs::create_dir(dir) {
        Ok(()) => restrict_dir(dir),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

const ENTRY_SUFFIX: &str = ".json";

/// On-disk form, mirroring [`LedgerEntry`] one field at a time. Timestamps are
/// explicit millis so the format does not depend on `serde`'s `SystemTime`
/// representation, matching [`crate::session_store`].
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    session_id: String,
    key: String,
    fingerprint: String,
    created_at_unix_ms: u64,
    #[serde(flatten)]
    state: StoredState,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum StoredState {
    Reserved,
    Completed { status: u16, body: String },
}

impl StoredEntry {
    fn from_entry(entry: &LedgerEntry) -> Self {
        Self {
            session_id: entry.session_id.clone(),
            key: entry.key.clone(),
            fingerprint: entry.fingerprint.clone(),
            created_at_unix_ms: entry
                .created_at
                .duration_since(UNIX_EPOCH)
                .map_or(0, saturating_millis),
            state: match &entry.state {
                EntryState::Reserved => StoredState::Reserved,
                EntryState::Completed(outcome) => StoredState::Completed {
                    status: outcome.status,
                    body: outcome.body.clone(),
                },
            },
        }
    }

    fn into_entry(self) -> LedgerEntry {
        LedgerEntry {
            session_id: self.session_id,
            key: self.key,
            fingerprint: self.fingerprint,
            created_at: UNIX_EPOCH + Duration::from_millis(self.created_at_unix_ms),
            state: match self.state {
                StoredState::Reserved => EntryState::Reserved,
                StoredState::Completed { status, body } => {
                    EntryState::Completed(RecordedOutcome { status, body })
                }
            },
        }
    }
}

fn entry_file_name(key: &str) -> String {
    format!("{}{ENTRY_SUFFIX}", hex_encode(key))
}

/// Hex-encode a caller-chosen string into a path-safe name. Hex doubles the
/// byte length, which is where R13's 120-byte key bound comes from: anything
/// longer would push the file name past `NAME_MAX`.
fn hex_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn hex_decode(name: &str) -> Option<String> {
    if !name.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(name.len() / 2);
    for pair in name.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    String::from_utf8(bytes).ok()
}

/// Create a file for writing, owner-read/write only (`0600`) on unix.
fn create_owner_only(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        File::create(path)
    }
}

/// Tighten a directory to owner-only (`0700`) on unix; best-effort elsewhere.
fn restrict_dir(dir: &Path) -> io::Result<()> {
    set_mode(dir, 0o700)
}

/// `open(2)`'s mode argument is filtered through the process umask, so a
/// permissive umask could still leave the entry group-readable. `chmod` is not,
/// which makes the mode exact rather than a ceiling.
fn restrict_file(path: &Path) -> io::Result<()> {
    set_mode(path, 0o600)
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let (_, _) = (path, mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> FileIdempotencyStore {
        FileIdempotencyStore::new(dir.join("ledger")).expect("file ledger")
    }

    fn completed(session_id: &str, key: &str, body: &str) -> LedgerEntry {
        LedgerEntry {
            state: EntryState::Completed(RecordedOutcome {
                status: 200,
                body: body.into(),
            }),
            ..LedgerEntry::reserved(session_id, key, code_fingerprint("main"))
        }
    }

    /// A write that fails must not leave a durability claim it never earned.
    /// The root fsync is what makes a session directory survive a crash; if a
    /// failed first write consumed the claim, every later write into that
    /// session would skip the fsync and its entries would vanish on power loss
    /// — the exact failure the sync exists to prevent, silently re-enabled by
    /// one transient error.
    #[tokio::test]
    async fn a_failed_write_leaves_the_session_directory_unclaimed() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        let dir_name = hex_encode("sid");

        // An unwritable root makes creating the session directory fail.
        fs::set_permissions(&store.root, fs::Permissions::from_mode(0o500)).unwrap();
        let entry = LedgerEntry::reserved("sid", "k1", code_fingerprint("main"));
        assert!(store.put(entry.clone()).await.is_err());
        assert!(
            !store.session_dir_cell(&dir_name).initialized(),
            "a failed write must not record the session directory as durable"
        );

        fs::set_permissions(&store.root, fs::Permissions::from_mode(0o700)).unwrap();
        store.put(entry.clone()).await.expect("retry after failure");
        assert!(store.session_dir_cell(&dir_name).initialized());
        assert_eq!(store.load("sid", "k1").await.unwrap(), Some(entry));
    }

    /// A purge removes the directory, so the claim that it is durable has to go
    /// with it — otherwise a session recreated under the same id writes into a
    /// directory whose own entry was never synced.
    #[tokio::test]
    async fn purging_a_session_releases_its_directory_claim() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        let dir_name = hex_encode("sid");
        store
            .put(LedgerEntry::reserved("sid", "k1", code_fingerprint("main")))
            .await
            .unwrap();
        assert!(store.session_dir_cell(&dir_name).initialized());

        store.purge_session("sid").await.unwrap();

        assert!(
            !store.session_dir_cell(&dir_name).initialized(),
            "a purged session must earn its root fsync again"
        );
    }

    #[tokio::test]
    async fn reserved_entry_round_trips_with_its_fingerprint() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        let entry = LedgerEntry::reserved("sid", "k1", code_fingerprint("main"));
        store.put(entry.clone()).await.unwrap();

        assert_eq!(store.load("sid", "k1").await.unwrap(), Some(entry));
    }

    #[tokio::test]
    async fn completed_entry_round_trips_byte_identically() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        // Awkward bytes on purpose: replay must return these unchanged.
        let body = "{\"result\":\"a\\nb \\\"quoted\\\" ☃\"}";
        let entry = completed("sid", "k1", body);
        store.put(entry.clone()).await.unwrap();

        let loaded = store.load("sid", "k1").await.unwrap().unwrap();
        assert_eq!(loaded, entry);
        assert_eq!(loaded.outcome().unwrap().body, body);
    }

    #[tokio::test]
    async fn put_replaces_a_reservation_with_its_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store
            .put(LedgerEntry::reserved("sid", "k1", code_fingerprint("main")))
            .await
            .unwrap();
        store.put(completed("sid", "k1", "done")).await.unwrap();

        let loaded = store.load("sid", "k1").await.unwrap().unwrap();
        assert_eq!(loaded.outcome().unwrap().body, "done");
    }

    #[tokio::test]
    async fn load_of_an_absent_key_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        assert_eq!(store.load("sid", "nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn purge_session_removes_every_state_and_spares_other_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store
            .put(LedgerEntry::reserved(
                "a",
                "reserved",
                code_fingerprint("main"),
            ))
            .await
            .unwrap();
        store.put(completed("a", "completed", "x")).await.unwrap();
        store.put(completed("b", "other", "y")).await.unwrap();

        store.purge_session("a").await.unwrap();

        assert_eq!(store.load("a", "reserved").await.unwrap(), None);
        assert_eq!(store.load("a", "completed").await.unwrap(), None);
        assert!(store.load("b", "other").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn remove_drops_one_entry_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store.put(completed("sid", "a", "x")).await.unwrap();
        store.put(completed("sid", "b", "y")).await.unwrap();

        store.remove("sid", "a").await.unwrap();
        assert_eq!(store.load("sid", "a").await.unwrap(), None);
        assert!(store.load("sid", "b").await.unwrap().is_some());
        // Removing an absent entry is a no-op, not an error.
        store.remove("sid", "a").await.unwrap();
        store.remove("never", "used").await.unwrap();
    }

    #[tokio::test]
    async fn purge_session_with_no_entries_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store.purge_session("never-used").await.unwrap();
    }

    #[tokio::test]
    async fn session_ids_reports_sessions_holding_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store.put(completed("a", "k", "x")).await.unwrap();
        store.put(completed("b", "k", "y")).await.unwrap();

        let mut ids = store.session_ids().await;
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn session_id_with_separators_stays_inside_the_store() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ledger");
        let store = FileIdempotencyStore::new(root.clone()).unwrap();
        store
            .put(completed("../../escape", "k", "x"))
            .await
            .unwrap();

        assert!(!tmp.path().join("escape").exists());
        let loaded = store.load("../../escape", "k").await.unwrap().unwrap();
        assert_eq!(loaded.session_id, "../../escape");
        assert_eq!(store.session_ids().await, vec!["../../escape".to_string()]);
    }

    #[tokio::test]
    async fn key_with_separators_stays_inside_the_session_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store.put(completed("sid", "../../k", "x")).await.unwrap();

        assert!(!tmp.path().join("k").exists());
        let loaded = store.load("sid", "../../k").await.unwrap().unwrap();
        assert_eq!(loaded.key, "../../k");
    }

    #[tokio::test]
    async fn a_key_at_the_length_bound_writes_successfully() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        let key = "k".repeat(120);
        store.put(completed("sid", &key, "x")).await.unwrap();

        // 120 bytes hex-encoded plus `.json` — comfortably inside NAME_MAX (255).
        assert_eq!(entry_file_name(&key).len(), 245);
        assert!(store.load("sid", &key).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_malformed_entry_errors_without_affecting_its_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(tmp.path());
        store.put(completed("sid", "good", "x")).await.unwrap();
        let session_dir = tmp.path().join("ledger").join(hex_encode("sid"));
        fs::write(session_dir.join(entry_file_name("bad")), b"{ not json").unwrap();

        // A corrupt entry must not read back as "no entry" — that would let the
        // request run a second time.
        assert!(store.load("sid", "bad").await.is_err());
        assert!(store.load("sid", "good").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_failed_write_returns_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ledger");
        let store = FileIdempotencyStore::new(root.clone()).unwrap();
        // Replace the store root with a regular file so creating the session
        // directory underneath it cannot succeed.
        fs::remove_dir_all(&root).unwrap();
        fs::write(&root, b"x").unwrap();

        assert!(store.put(completed("sid", "k", "x")).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn store_root_session_dir_and_entry_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ledger");
        let store = FileIdempotencyStore::new(root.clone()).unwrap();
        store.put(completed("sid", "k", "x")).await.unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let session_dir = root.join(hex_encode("sid"));
        assert_eq!(mode(&root), 0o700, "store root must be owner-only");
        assert_eq!(mode(&session_dir), 0o700, "session dir must be owner-only");
        assert_eq!(
            mode(&session_dir.join(entry_file_name("k"))),
            0o600,
            "entry file must be owner-only"
        );
    }

    #[test]
    fn fingerprint_distinguishes_code_and_is_stable() {
        assert_eq!(code_fingerprint("a"), code_fingerprint("a"));
        assert_ne!(code_fingerprint("a"), code_fingerprint("b"));
        assert_eq!(code_fingerprint("a").len(), 64);
    }

    #[test]
    fn hex_round_trips_through_decode() {
        for value in ["sid", "../../escape", "a:b:1", "☃"] {
            assert_eq!(hex_decode(&hex_encode(value)).as_deref(), Some(value));
        }
        assert_eq!(hex_decode("odd"), None);
        assert_eq!(hex_decode("zz"), None);
    }
}
