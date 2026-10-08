//! Session persistence implementations and legacy file decoding.
use crate::blueprint::StoreError;
use crate::session_store::{
    ClosedReason, DurableSessionStore, RootVfsType, SessionCleanup, SessionRecord, SessionStatus,
    sanitize_mcp_state,
};
use rmcp::transport::streamable_http_server::session::SessionState as McpSessionState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub(crate) mod credentials;
pub(crate) mod repository;
pub(crate) mod sqlite;

/// Ephemeral store for tests and the no-persistence fallback.
#[derive(Default)]
pub struct InMemoryDurableSessionStore {
    inner: Mutex<HashMap<String, SessionRecord>>,
    cleanup: Mutex<HashMap<String, SessionCleanup>>,
}

#[async_trait::async_trait]
impl DurableSessionStore for InMemoryDurableSessionStore {
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError> {
        let mut state = self.lock();
        check_revision(state.get(&record.session_id), &record)?;
        let mut record = record;
        record.revision = next_revision(record.revision)?;
        state.insert(record.session_id.clone(), record);
        Ok(())
    }

    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self.lock().get(session_id).cloned())
    }

    async fn remove(&self, session_id: &str) -> Result<(), StoreError> {
        self.lock().remove(session_id);
        Ok(())
    }

    async fn schedule_cleanup(&self, task: SessionCleanup) -> Result<(), StoreError> {
        // Accepted poisoned-lock exception: unfinished task mutation must not be recovered.
        self.cleanup
            .lock()
            .expect("cleanup task lock poisoned")
            .insert(task.session_id.clone(), task);
        Ok(())
    }
    async fn pending_cleanup(&self) -> Result<Vec<SessionCleanup>, StoreError> {
        let mut tasks: HashMap<_, _> = self
            .load_all()
            .await?
            .iter()
            .filter(|r| r.cleanup_pending)
            .map(|r| (r.session_id.clone(), SessionCleanup::from_record(r)))
            .collect();
        // Accepted poisoned-lock exception, as in schedule_cleanup.
        tasks.extend(
            self.cleanup
                .lock()
                .expect("cleanup task lock poisoned")
                .clone(),
        );
        Ok(tasks.into_values().collect())
    }
    async fn complete_cleanup(&self, id: &str) -> Result<(), StoreError> {
        if let Some(record) = self.lock().get_mut(id) {
            record.cleanup_pending = false;
        }
        // Accepted poisoned-lock exception, as in schedule_cleanup.
        self.cleanup
            .lock()
            .expect("cleanup task lock poisoned")
            .remove(id);
        Ok(())
    }

    async fn load_all(&self) -> Result<Vec<SessionRecord>, StoreError> {
        Ok(self.lock().values().cloned().collect())
    }
}

impl InMemoryDurableSessionStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionRecord>> {
        // Poison means a panic may have interrupted a session-record mutation.
        // AGENTS.md permits poisoned-lock panics rather than recovering potentially
        // partial session state; it does not permit the panic that caused poisoning.
        self.inner.lock().expect("store lock poisoned")
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
    fn durable(&self) -> bool {
        true
    }
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError> {
        let existing = self.load(&record.session_id).await?;
        check_revision(existing.as_ref(), &record)?;
        let mut record = record;
        record.revision = next_revision(record.revision)?;
        let bytes = serde_json::to_vec_pretty(&StoredRecord::from_record(&record))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        self.atomic_write(&record_file_name(&record.session_id), &bytes)
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        let path = self.dir.join(record_file_name(session_id));
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<StoredRecord>(&bytes)
                .map_err(|e| StoreError::Io(e.to_string()))
                .and_then(|stored| stored.into_record().map(Some)),
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

    async fn schedule_cleanup(&self, task: SessionCleanup) -> Result<(), StoreError> {
        let queue =
            Self::new(self.dir.join("cleanup")).map_err(|e| StoreError::Io(e.to_string()))?;
        let bytes = serde_json::to_vec(&task).map_err(|e| StoreError::Io(e.to_string()))?;
        queue
            .atomic_write(&record_file_name(&task.session_id), &bytes)
            .map_err(|e| StoreError::Io(e.to_string()))
    }
    async fn pending_cleanup(&self) -> Result<Vec<SessionCleanup>, StoreError> {
        let mut tasks: HashMap<_, _> = self
            .load_all()
            .await?
            .iter()
            .filter(|r| r.cleanup_pending)
            .map(|r| (r.session_id.clone(), SessionCleanup::from_record(r)))
            .collect();
        let directory = self.dir.join("cleanup");
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(tasks.into_values().collect());
            }
            Err(e) => return Err(StoreError::Io(e.to_string())),
        };
        for entry in entries {
            let path = entry.map_err(|e| StoreError::Io(e.to_string()))?.path();
            if !is_record_file(&path) {
                continue;
            }
            let bytes = fs::read(path).map_err(|e| StoreError::Io(e.to_string()))?;
            let task: SessionCleanup =
                serde_json::from_slice(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            tasks.insert(task.session_id.clone(), task);
        }
        Ok(tasks.into_values().collect())
    }
    async fn complete_cleanup(&self, id: &str) -> Result<(), StoreError> {
        if let Some(mut record) = self.load(id).await? {
            record.cleanup_pending = false;
            self.put(record).await?;
        }
        let directory = self.dir.join("cleanup");
        match fs::remove_file(directory.join(record_file_name(id))) {
            Ok(()) => File::open(directory)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| StoreError::Io(e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    // Skip malformed JSON files so one bad legacy file does not prevent boot.
    // I/O failures and invalid decoded records still propagate.
    async fn load_all(&self) -> Result<Vec<SessionRecord>, StoreError> {
        let entries = fs::read_dir(&self.dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let mut records = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| StoreError::Io(e.to_string()))?.path();
            if !is_record_file(&path) {
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| StoreError::Io(e.to_string()))?;
            match serde_json::from_slice::<StoredRecord>(&bytes) {
                Ok(stored) => records.push(stored.into_record()?),
                Err(e) => {
                    tracing::warn!(path = %path.display(), %e, "skipping malformed session record");
                }
            }
        }
        Ok(records)
    }
}

fn next_revision(revision: i64) -> Result<i64, StoreError> {
    revision
        .checked_add(1)
        .ok_or_else(|| StoreError::Io("session revision exhausted".into()))
}

pub(crate) fn check_revision(
    existing: Option<&SessionRecord>,
    proposed: &SessionRecord,
) -> Result<(), StoreError> {
    proposed.validate_lifecycle()?;
    if existing.map_or(0, |record| record.revision) != proposed.revision {
        return Err(StoreError::Io("session revision conflict".into()));
    }
    if existing.is_some_and(|record| {
        record.status == SessionStatus::Closed
            && (proposed.status == SessionStatus::Active
                || record.closed_reason.and_then(ClosedReason::as_storage)
                    != proposed.closed_reason.and_then(ClosedReason::as_storage))
    }) {
        return Err(StoreError::Io(
            "closed session lifecycle cannot change".into(),
        ));
    }
    Ok(())
}

const RECORD_SUFFIX: &str = ".json";

/// On-disk form: durations and timestamps as explicit millis for a format that
/// doesn't depend on `serde`'s `SystemTime`/`Duration` representation.
#[derive(Serialize, Deserialize)]
struct StoredRecord {
    #[serde(default)]
    revision: i64,
    #[serde(default)]
    retired: bool,
    #[serde(default)]
    closed_reason: Option<ClosedReason>,
    #[serde(default)]
    cleanup_pending: bool,
    #[serde(default, alias = "workspace")]
    root_vfs_path: Option<PathBuf>,
    #[serde(default)]
    sealed_bindings: Option<Vec<u8>>,
    session_id: String,
    blueprint_name: String,
    idle_timeout_ms: u64,
    last_activity_unix_ms: u64,
    #[serde(default)]
    owns_vfs_dir: bool,
    #[serde(default)]
    root_vfs_type: Option<RootVfsType>,
    #[serde(default)]
    mcp_state: Option<McpSessionState>,
    /// Defaulted so records written before session variables existed still load.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    variables: BTreeMap<String, String>,
}

impl StoredRecord {
    fn from_record(record: &SessionRecord) -> Self {
        let mut mcp_state = record.mcp_state.clone();
        if let Some(state) = &mut mcp_state {
            sanitize_mcp_state(state);
        }
        Self {
            revision: record.revision,
            retired: record.status == SessionStatus::Closed,
            closed_reason: record.closed_reason,
            cleanup_pending: record.cleanup_pending,
            root_vfs_path: record.root_vfs_path.clone(),
            sealed_bindings: record.sealed_bindings.clone(),
            session_id: record.session_id.clone(),
            blueprint_name: record.blueprint_name.clone(),
            idle_timeout_ms: saturating_ms(record.idle_timeout),
            last_activity_unix_ms: record
                .last_activity
                .duration_since(UNIX_EPOCH)
                .map_or(0, saturating_ms),
            owns_vfs_dir: record.root_vfs_type == RootVfsType::PerSession,
            root_vfs_type: (!record.legacy_root_unresolved).then_some(record.root_vfs_type),
            mcp_state,
            variables: record.variables.clone(),
        }
    }

    fn into_record(mut self) -> Result<SessionRecord, StoreError> {
        if let Some(state) = &mut self.mcp_state {
            sanitize_mcp_state(state);
        }
        let record = SessionRecord {
            revision: self.revision,
            status: if self.retired {
                SessionStatus::Closed
            } else {
                SessionStatus::Active
            },
            closed_reason: self.closed_reason,
            cleanup_pending: self.cleanup_pending,
            root_vfs_path: self.root_vfs_path,
            legacy_root_unresolved: self.root_vfs_type.is_none() && !self.owns_vfs_dir,
            sealed_bindings: self.sealed_bindings,
            ephemeral_bindings: None,
            session_id: self.session_id,
            blueprint_name: self.blueprint_name,
            idle_timeout: Duration::from_millis(self.idle_timeout_ms),
            last_activity: UNIX_EPOCH
                .checked_add(Duration::from_millis(self.last_activity_unix_ms))
                .ok_or_else(|| StoreError::Io("invalid session activity timestamp".into()))?,
            root_vfs_type: self.root_vfs_type.unwrap_or(if self.owns_vfs_dir {
                RootVfsType::PerSession
            } else {
                RootVfsType::None
            }),
            mcp_state: self.mcp_state,
            variables: self.variables,
        };
        record.validate_lifecycle()?;
        Ok(record)
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

    #[test]
    fn unknown_reason_preserves_legacy_closure_without_allowing_a_new_cause() {
        let existing = SessionRecord {
            status: SessionStatus::Closed,
            ..Default::default()
        };
        let mut proposed = existing.clone();
        proposed.closed_reason = Some(ClosedReason::Unknown);
        assert!(check_revision(Some(&existing), &proposed).is_ok());
        assert!(check_revision(Some(&proposed), &existing).is_ok());
        proposed.closed_reason = Some(ClosedReason::Deleted);
        assert!(check_revision(Some(&existing), &proposed).is_err());
    }

    fn record(session_id: &str) -> SessionRecord {
        SessionRecord {
            session_id: session_id.into(),
            blueprint_name: "bp".into(),
            idle_timeout: Duration::from_secs(3600),
            // Truncated to ms so the on-disk round-trip is exact.
            last_activity: UNIX_EPOCH + Duration::from_millis(1_700_000_000_000),
            root_vfs_type: RootVfsType::PerSession,
            mcp_state: None,
            variables: BTreeMap::new(),
            ..SessionRecord::default()
        }
    }

    #[tokio::test]
    async fn put_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        let rec = record("sid-1");
        store.put(rec.clone()).await.unwrap();

        let loaded = store.load_all().await.expect("list sessions");
        assert_eq!(loaded, vec![rec]);
    }

    #[tokio::test]
    async fn variables_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        let mut rec = record("sid-vars");
        rec.variables = BTreeMap::from([("tenant".to_string(), "u_42".to_string())]);
        store.put(rec.clone()).await.unwrap();
        assert_eq!(store.load_all().await.expect("list sessions"), vec![rec]);
    }

    #[tokio::test]
    async fn put_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("sid")).await.unwrap();
        let mut updated = store.load("sid").await.unwrap().unwrap();
        updated.blueprint_name = "other".into();
        store.put(updated.clone()).await.unwrap();

        let loaded = store.load_all().await.expect("list sessions");
        assert_eq!(loaded, vec![updated]);
    }

    #[tokio::test]
    async fn remove_deletes_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("sid")).await.unwrap();
        store.remove("sid").await.unwrap();
        assert!(store.load_all().await.expect("list sessions").is_empty());
        // Removing an absent record is a no-op, not an error.
        store.remove("sid").await.unwrap();
    }

    #[tokio::test]
    async fn load_skips_malformed_file_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("good")).await.unwrap();
        fs::write(dir.path().join("deadbeef.json"), b"{ not json").unwrap();

        let loaded = store.load_all().await.expect("list sessions");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, "good");
    }

    #[tokio::test]
    async fn session_id_with_separators_stays_inside_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileDurableSessionStore::new(dir.path().to_path_buf()).unwrap();
        store.put(record("../../escape")).await.unwrap();

        // The file landed flat under `dir`, and the id round-trips intact.
        let loaded = store.load_all().await.expect("list sessions");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, "../../escape");
    }

    #[tokio::test]
    async fn enumeration_reports_unavailable_file_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        let store = FileDurableSessionStore::new(root.clone()).unwrap();
        fs::remove_dir(&root).unwrap();

        assert!(matches!(store.load_all().await, Err(StoreError::Io(_))));
    }
}

pub(crate) mod audit_log;
pub(crate) mod cleanup_queue;
pub(crate) mod idempotency_records;
pub(crate) mod workspaces;
