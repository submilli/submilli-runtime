//! Durable persistence of session lifecycle metadata so resume and idle reaping
//! survive a server restart.
//!
//! The hot path keeps an in-memory cache (see [`SessionManager`]); this store is
//! a write-through mirror plus the boot-time source of truth. The file backend
//! writes one JSON file per session, atomically (temp → fsync → rename → fsync
//! dir) exactly like [`FileBlueprintStore`]. The session id is hex-encoded into
//! the file name so an arbitrary client-chosen id (which may contain path
//! separators) can never escape `dir`; the real id lives in the file body and is
//! read back on load.
//!
//! [`SessionManager`]: crate::session_manager::SessionManager
//! [`FileBlueprintStore`]: crate::blueprint::FileBlueprintStore

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use rmcp::transport::streamable_http_server::session::SessionState as McpSessionState;

use crate::blueprint::StoreError;

/// Lifecycle metadata for one session — everything the reaper and resume need
/// after a restart. The HTTP client and live VFS handle are not persisted; they
/// rebuild lazily on first use.
#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub session_id: String,
    pub blueprint_name: String,
    pub idle_timeout: Duration,
    pub last_activity: SystemTime,
    /// `true` only for `per_session`, the one mode that owns a directory the
    /// reaper wipes.
    pub owns_vfs_dir: bool,
    /// rmcp's original `initialize` params. The streamable-HTTP layer replays
    /// these to rebuild its in-memory MCP session worker after a restart.
    pub mcp_state: Option<McpSessionState>,
    /// Caller-supplied `${vars.NAME}` bindings, bound once at session init.
    /// Persisted so a session resumed after a restart keeps its variable scoping.
    pub variables: BTreeMap<String, String>,
}

impl PartialEq for SessionRecord {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.blueprint_name == other.blueprint_name
            && self.idle_timeout == other.idle_timeout
            && self.last_activity == other.last_activity
            && self.owns_vfs_dir == other.owns_vfs_dir
            && self.mcp_state.is_some() == other.mcp_state.is_some()
            && self.variables == other.variables
    }
}

impl Eq for SessionRecord {}

/// Async to mirror [`BlueprintStore`](crate::blueprint::BlueprintStore) so a
/// DB-backed implementation can be swapped in without reshaping call sites.
#[async_trait::async_trait]
pub trait DurableSessionStore: Send + Sync + 'static {
    /// Insert or replace the record for `record.session_id`.
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError>;
    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError>;
    async fn remove(&self, session_id: &str) -> Result<(), StoreError>;
    /// Every persisted record. Malformed entries are skipped, not fatal — one
    /// bad file must not stop the server from booting.
    async fn load_all(&self) -> Vec<SessionRecord>;
}

/// Ephemeral store for tests and the no-persistence fallback.
#[derive(Default)]
pub struct InMemoryDurableSessionStore {
    inner: Mutex<HashMap<String, SessionRecord>>,
}

#[async_trait::async_trait]
impl DurableSessionStore for InMemoryDurableSessionStore {
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError> {
        self.lock().insert(record.session_id.clone(), record);
        Ok(())
    }

    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self.lock().get(session_id).cloned())
    }

    async fn remove(&self, session_id: &str) -> Result<(), StoreError> {
        self.lock().remove(session_id);
        Ok(())
    }

    async fn load_all(&self) -> Vec<SessionRecord> {
        self.lock().values().cloned().collect()
    }
}

impl InMemoryDurableSessionStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionRecord>> {
        self.inner.lock().expect("session store mutex poisoned")
    }
}

/// Crash-safe, file-backed session store: one `<hex(session_id)>.json` per
/// session, overwritten in place. Unlike the blueprint store there is no
/// revision history — `last_activity` changes on every touch, so retaining old
/// versions would only accumulate garbage.
pub struct FileDurableSessionStore {
    dir: PathBuf,
}

impl FileDurableSessionStore {
    /// Open the store, creating `dir` if absent.
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Write `file_name` durably: temp → fsync → atomic rename → fsync dir.
    fn atomic_write(&self, file_name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!(".{file_name}.tmp"));
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.dir.join(file_name))?;
        File::open(&self.dir)?.sync_all()
    }
}

#[async_trait::async_trait]
impl DurableSessionStore for FileDurableSessionStore {
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(&StoredRecord::from_record(&record))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        self.atomic_write(&record_file_name(&record.session_id), &bytes)
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        let path = self.dir.join(record_file_name(session_id));
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<StoredRecord>(&bytes)
                .map(|stored| Some(stored.into_record()))
                .map_err(|e| StoreError::Io(e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    async fn remove(&self, session_id: &str) -> Result<(), StoreError> {
        match fs::remove_file(self.dir.join(record_file_name(session_id))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    async fn load_all(&self) -> Vec<SessionRecord> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                tracing::warn!(dir = %self.dir.display(), %e, "cannot read session store dir");
                return Vec::new();
            }
        };
        let mut records = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_record_file(&path) {
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => match serde_json::from_slice::<StoredRecord>(&bytes) {
                    Ok(stored) => records.push(stored.into_record()),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), %e, "skipping malformed session record");
                    }
                },
                Err(e) => {
                    tracing::warn!(path = %path.display(), %e, "skipping unreadable session record");
                }
            }
        }
        records
    }
}

const RECORD_SUFFIX: &str = ".json";

/// On-disk form: durations and timestamps as explicit millis for a format that
/// doesn't depend on `serde`'s `SystemTime`/`Duration` representation.
#[derive(Serialize, Deserialize)]
struct StoredRecord {
    session_id: String,
    blueprint_name: String,
    idle_timeout_ms: u64,
    last_activity_unix_ms: u64,
    owns_vfs_dir: bool,
    #[serde(default)]
    mcp_state: Option<McpSessionState>,
    /// Defaulted so records written before session variables existed still load.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    variables: BTreeMap<String, String>,
}

impl StoredRecord {
    fn from_record(record: &SessionRecord) -> Self {
        Self {
            session_id: record.session_id.clone(),
            blueprint_name: record.blueprint_name.clone(),
            idle_timeout_ms: saturating_ms(record.idle_timeout),
            last_activity_unix_ms: record
                .last_activity
                .duration_since(UNIX_EPOCH)
                .map_or(0, saturating_ms),
            owns_vfs_dir: record.owns_vfs_dir,
            mcp_state: record.mcp_state.clone(),
            variables: record.variables.clone(),
        }
    }

    fn into_record(self) -> SessionRecord {
        SessionRecord {
            session_id: self.session_id,
            blueprint_name: self.blueprint_name,
            idle_timeout: Duration::from_millis(self.idle_timeout_ms),
            last_activity: UNIX_EPOCH + Duration::from_millis(self.last_activity_unix_ms),
            owns_vfs_dir: self.owns_vfs_dir,
            mcp_state: self.mcp_state,
            variables: self.variables,
        }
    }
}

fn saturating_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Hex-encode the session id into a flat, path-safe file name. The real id is
/// stored in the body, so this never has to be decoded.
fn record_file_name(session_id: &str) -> String {
    let mut name = String::with_capacity(session_id.len() * 2 + RECORD_SUFFIX.len());
    for byte in session_id.bytes() {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(RECORD_SUFFIX);
    name
}

fn is_record_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| !n.starts_with('.') && n.ends_with(RECORD_SUFFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session_id: &str) -> SessionRecord {
        SessionRecord {
            session_id: session_id.into(),
            blueprint_name: "bp".into(),
            idle_timeout: Duration::from_secs(3600),
            // Truncated to ms so the on-disk round-trip is exact.
            last_activity: UNIX_EPOCH + Duration::from_millis(1_700_000_000_000),
            owns_vfs_dir: true,
            mcp_state: None,
            variables: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn put_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        let rec = record("sid-1");
        store.put(rec.clone()).await.unwrap();

        let loaded = store.load_all().await;
        assert_eq!(loaded, vec![rec]);
    }

    #[tokio::test]
    async fn variables_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        let mut rec = record("sid-vars");
        rec.variables = BTreeMap::from([("tenant".to_string(), "u_42".to_string())]);
        store.put(rec.clone()).await.unwrap();
        assert_eq!(store.load_all().await, vec![rec]);
    }

    #[tokio::test]
    async fn put_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("sid")).await.unwrap();
        let mut updated = record("sid");
        updated.blueprint_name = "other".into();
        store.put(updated.clone()).await.unwrap();

        let loaded = store.load_all().await;
        assert_eq!(loaded, vec![updated]);
    }

    #[tokio::test]
    async fn remove_deletes_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("sid")).await.unwrap();
        store.remove("sid").await.unwrap();
        assert!(store.load_all().await.is_empty());
        // Removing an absent record is a no-op, not an error.
        store.remove("sid").await.unwrap();
    }

    #[tokio::test]
    async fn load_skips_malformed_file_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("good")).await.unwrap();
        fs::write(dir.path().join("deadbeef.json"), b"{ not json").unwrap();

        let loaded = store.load_all().await;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, "good");
    }

    #[tokio::test]
    async fn session_id_with_separators_stays_inside_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("../../escape")).await.unwrap();

        // The file landed flat under `dir`, and the id round-trips intact.
        let loaded = store.load_all().await;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, "../../escape");
    }
}
