//! Authoritative session records and compatibility JSON import.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rmcp::transport::streamable_http_server::session::SessionState as McpSessionState;

use crate::blueprint::StoreError;

pub use crate::domain::session::{ClosedReason, RootVfsType};

/// Flat storage representation; the domain status carries its closure reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Active,
    Closed,
}

pub use crate::adapters::session::{
    FileDurableSessionStore, InMemoryDurableSessionStore, sqlite::SqliteSessionStore,
};

impl SessionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            _ => Err(StoreError::Io("invalid session status".into())),
        }
    }
}

impl ClosedReason {
    pub(crate) fn as_storage(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Deleted => Some("deleted"),
            Self::Expired => Some("expired"),
            Self::BlueprintRemoved => Some("blueprint_removed"),
        }
    }

    pub(crate) fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "deleted" => Ok(Self::Deleted),
            "expired" => Ok(Self::Expired),
            "blueprint_removed" => Ok(Self::BlueprintRemoved),
            _ => Err(StoreError::Io("invalid session closed reason".into())),
        }
    }
}

impl RootVfsType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ephemeral => "ephemeral",
            Self::PerSession => "per_session",
            Self::Named => "named",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "none" => Ok(Self::None),
            "ephemeral" => Ok(Self::Ephemeral),
            "per_session" => Ok(Self::PerSession),
            "named" => Ok(Self::Named),
            _ => Err(StoreError::Io("invalid session root VFS type".into())),
        }
    }
}

impl From<&submilli_blueprint::VfsConfig> for RootVfsType {
    fn from(config: &submilli_blueprint::VfsConfig) -> Self {
        match config {
            submilli_blueprint::VfsConfig::None => Self::None,
            submilli_blueprint::VfsConfig::Ephemeral { .. } => Self::Ephemeral,
            submilli_blueprint::VfsConfig::PerSession { .. } => Self::PerSession,
            submilli_blueprint::VfsConfig::Named { .. } => Self::Named,
        }
    }
}

/// Persistence snapshot used by storage adapters and legacy import.
/// Application changes go through the session aggregate and repository;
/// this row shape is not an entity or an application command.
#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub revision: i64,
    pub status: SessionStatus,
    /// Legacy records and orphan cleanup entries may have no recorded reason.
    pub closed_reason: Option<ClosedReason>,
    pub cleanup_pending: bool,
    pub root_vfs_path: Option<PathBuf>,
    /// Only legacy input can lack the root mode; resolved bindings clear this marker.
    pub legacy_root_unresolved: bool,
    pub sealed_bindings: Option<Vec<u8>>,
    /// Only explicit non-durable stores may retain plaintext bindings.
    pub ephemeral_bindings: Option<BTreeMap<String, String>>,
    pub session_id: String,
    pub blueprint_name: String,
    pub idle_timeout: Duration,
    pub last_activity: SystemTime,
    /// The root filesystem mode; only `PerSession` permits directory cleanup.
    pub root_vfs_type: RootVfsType,
    /// rmcp's original `initialize` params. The streamable-HTTP layer replays
    /// these to rebuild its in-memory MCP session worker after a restart.
    pub mcp_state: Option<McpSessionState>,
    /// Caller-supplied `${vars.NAME}` bindings, bound once at session init.
    /// Persisted so a session resumed after a restart keeps its variable scoping.
    pub variables: BTreeMap<String, String>,
}

impl SessionRecord {
    pub(crate) fn validate_lifecycle(&self) -> Result<(), StoreError> {
        if self.status == SessionStatus::Active && self.closed_reason.is_some() {
            return Err(StoreError::Io(
                "active session cannot have a closed reason".into(),
            ));
        }
        Ok(())
    }
}

impl Default for SessionRecord {
    fn default() -> Self {
        Self {
            revision: 0,
            status: SessionStatus::Active,
            closed_reason: None,
            cleanup_pending: false,
            root_vfs_path: None,
            legacy_root_unresolved: false,
            sealed_bindings: None,
            ephemeral_bindings: None,
            session_id: String::new(),
            blueprint_name: String::new(),
            idle_timeout: Duration::ZERO,
            last_activity: UNIX_EPOCH,
            root_vfs_type: RootVfsType::None,
            mcp_state: None,
            variables: BTreeMap::new(),
        }
    }
}

impl PartialEq for SessionRecord {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.blueprint_name == other.blueprint_name
            && self.idle_timeout == other.idle_timeout
            && self.last_activity == other.last_activity
            && self.root_vfs_type == other.root_vfs_type
            && self.mcp_state.is_some() == other.mcp_state.is_some()
            && self.variables == other.variables
    }
}

impl Eq for SessionRecord {}

pub use crate::application::sessions::ports::SessionCleanup;

impl SessionCleanup {
    pub(crate) fn from_record(record: &SessionRecord) -> Self {
        Self {
            session_id: record.session_id.clone(),
            folder: if record.root_vfs_type == RootVfsType::PerSession {
                record.root_vfs_path.clone()
            } else {
                None
            },
        }
    }
}

/// Async to mirror [`BlueprintStore`](crate::blueprint::BlueprintStore) so a
/// DB-backed implementation can be swapped in without reshaping call sites.
#[async_trait::async_trait]
pub trait DurableSessionStore: Send + Sync + 'static {
    /// Shared transaction owner, used only when composing persistence adapters.
    fn database(&self) -> Option<std::sync::Arc<crate::database::ServerDatabase>> {
        None
    }

    fn durable(&self) -> bool {
        false
    }
    async fn initialize(&self) -> Result<(), StoreError> {
        Ok(())
    }
    async fn now(&self) -> Result<SystemTime, StoreError> {
        Ok(SystemTime::now())
    }

    /// Insert or replace the record for `record.session_id`.
    async fn put(&self, record: SessionRecord) -> Result<(), StoreError>;
    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError>;
    async fn remove(&self, session_id: &str) -> Result<(), StoreError>;
    async fn schedule_cleanup(&self, task: SessionCleanup) -> Result<(), StoreError>;
    async fn pending_cleanup(&self) -> Result<Vec<SessionCleanup>, StoreError>;
    async fn cleanup_task(&self, id: &str) -> Result<Option<SessionCleanup>, StoreError> {
        Ok(self
            .pending_cleanup()
            .await?
            .into_iter()
            .find(|task| task.session_id == id))
    }
    async fn active_ids(&self) -> Result<Vec<String>, StoreError> {
        Ok(self
            .load_all()
            .await?
            .into_iter()
            .filter(|record| record.status == SessionStatus::Active)
            .map(|record| record.session_id)
            .collect())
    }

    async fn complete_cleanup(&self, id: &str) -> Result<(), StoreError>;
    /// Enumerate persisted records, propagating storage and decoding failures.
    /// Compatibility file adapters may skip malformed JSON entries.
    async fn load_all(&self) -> Result<Vec<SessionRecord>, StoreError>;
    async fn active_count(&self) -> Result<usize, StoreError> {
        Ok(self
            .load_all()
            .await?
            .iter()
            .filter(|record| record.status == SessionStatus::Active)
            .count())
    }
    async fn for_blueprint(&self, name: &str) -> Result<Vec<SessionRecord>, StoreError> {
        Ok(self
            .load_all()
            .await?
            .into_iter()
            .filter(|record| record.blueprint_name == name)
            .collect())
    }
    async fn expiry_candidates(&self, now: SystemTime) -> Result<Vec<SessionRecord>, StoreError> {
        Ok(self
            .load_all()
            .await?
            .into_iter()
            .filter(|record| {
                record.status == SessionStatus::Active
                    && crate::domain::session::SessionLifetime::new(
                        record.idle_timeout,
                        record.last_activity,
                    )
                    .expired_at(now)
            })
            .collect())
    }
}

pub(crate) fn sanitize_mcp_state(state: &mut McpSessionState) {
    if let Some(meta) = &mut state.initialize_params.meta {
        meta.0.remove("secrets");
    }
}
