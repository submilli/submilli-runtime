//! Application-facing session repository and persistence mapping.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::credentials::{CredentialCodec, CredentialError, SessionCredentials};
use crate::blueprint::StoreError;
use crate::domain::session::*;
use crate::session_store::{DurableSessionStore, SessionRecord, SessionStatus as RecordStatus};
use submilli_blueprint::HarnessSecretBindings;

pub struct SessionPersistence {
    store: Arc<dyn DurableSessionStore>,
    session_root: PathBuf,
}

/// Optimistic concurrency and transport/credential attachments belong to persistence.
/// The domain value is the only authority for changes to session lifecycle and binding.
pub struct LoadedSession {
    session: Session,
    stored: SessionRecord,
}

impl LoadedSession {
    pub(crate) fn restore(stored: SessionRecord, root: &Path) -> Result<Self, StoreError> {
        let session = restore_domain(&stored, root)?;
        Ok(Self { session, stored })
    }

    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn resolve_root(&mut self, root: RootVfs) -> Result<(), SessionRuleError> {
        self.session.resolve_root(root)
    }
    pub fn into_record(self) -> SessionRecord {
        let mut record = self.stored;
        write_domain(&mut record, &self.session);
        if matches!(self.session.status(), SessionStatus::Closed(_)) {
            record.sealed_bindings = None;
            record.ephemeral_bindings = None;
            record.mcp_state = None;
        }
        record
    }
    /// Resolve execution inputs from the same stored revision.
    pub fn execution_bindings(
        &self,
        codec: &CredentialCodec,
    ) -> Result<(SessionBinding, Option<Arc<HarnessSecretBindings>>), CredentialError> {
        let credentials = match (
            &self.stored.sealed_bindings,
            &self.stored.ephemeral_bindings,
        ) {
            (Some(blob), _) => SessionCredentials::Encrypted(blob.clone()),
            (None, Some(bindings)) => SessionCredentials::Transient(bindings.clone()),
            (None, None) => SessionCredentials::Absent,
        };
        let secrets = codec.open(self.session.id().as_str(), &credentials)?;
        Ok((self.session.binding().clone(), secrets))
    }

    pub fn close(&mut self, reason: ClosedReason) -> bool {
        if !self.session.close(reason) {
            return false;
        }
        self.stored.cleanup_pending = true;
        self.stored.sealed_bindings = None;
        self.stored.ephemeral_bindings = None;
        self.stored.mcp_state = None;
        true
    }
}

impl SessionPersistence {
    pub fn new(store: Arc<dyn DurableSessionStore>, session_root: PathBuf) -> Self {
        Self {
            store,
            session_root,
        }
    }
    pub async fn find(&self, id: &str) -> Result<Option<LoadedSession>, StoreError> {
        self.store
            .load(id)
            .await?
            .map(|stored| self.restore(stored))
            .transpose()
    }
    pub fn restore(&self, stored: SessionRecord) -> Result<LoadedSession, StoreError> {
        LoadedSession::restore(stored, &self.session_root)
    }
    pub async fn save(&self, loaded: LoadedSession) -> Result<(), StoreError> {
        self.store.put(loaded.into_record()).await
    }
}

pub(crate) fn restore_domain(
    record: &SessionRecord,
    session_root: &Path,
) -> Result<Session, StoreError> {
    let id = SessionId::parse(record.session_id.clone()).map_err(rule_error)?;
    let binding = SessionBinding::new(record.blueprint_name.clone(), record.variables.clone())
        .map_err(rule_error)?;
    let status = match (record.status, record.closed_reason) {
        (RecordStatus::Active, None) => SessionStatus::Active,
        (RecordStatus::Active, Some(_)) => {
            return Err(StoreError::Io("active session has a closure reason".into()));
        }
        (RecordStatus::Closed, reason) => {
            SessionStatus::Closed(reason.unwrap_or(ClosedReason::Unknown))
        }
    };
    let root = if record.legacy_root_unresolved {
        RootVfs::LegacyUnresolved
    } else {
        match record.root_vfs_type {
            RootVfsType::None => RootVfs::None,
            RootVfsType::Ephemeral => RootVfs::Ephemeral,
            RootVfsType::PerSession => RootVfs::PerSession {
                path: record
                    .root_vfs_path
                    .clone()
                    .unwrap_or_else(|| session_root.join(id.as_str())),
            },
            RootVfsType::Named => match &record.root_vfs_path {
                Some(path) => RootVfs::Named { path: path.clone() },
                None => return Err(StoreError::Io("named session root path is missing".into())),
            },
        }
    };
    Session::restore(
        id,
        binding,
        root,
        SessionLifetime::new(record.idle_timeout, record.last_activity),
        status,
        if record.sealed_bindings.is_some() || record.ephemeral_bindings.is_some() {
            CredentialBinding::Persisted
        } else {
            CredentialBinding::Absent
        },
    )
    .map_err(rule_error)
}

pub(crate) fn close_record(
    record: SessionRecord,
    reason: ClosedReason,
    session_root: &Path,
) -> Result<SessionRecord, StoreError> {
    let session = restore_domain(&record, session_root)?;
    let mut loaded = LoadedSession {
        session,
        stored: record,
    };
    loaded.close(reason);
    Ok(loaded.into_record())
}

pub(crate) fn persist_session(
    session: Session,
    previous: Option<SessionRecord>,
    codec: &CredentialCodec,
    durable: bool,
) -> Result<SessionRecord, StoreError> {
    let was_active = previous
        .as_ref()
        .is_some_and(|record| record.status == RecordStatus::Active);
    let mut record = previous.unwrap_or_default();
    write_domain(&mut record, &session);
    match session.credentials() {
        CredentialBinding::Persisted => {}
        CredentialBinding::Absent => {
            record.sealed_bindings = None;
            record.ephemeral_bindings = None;
        }
        CredentialBinding::Supplied(bindings) => {
            match codec
                .seal(session.id().as_str(), bindings, durable)
                .map_err(|error| StoreError::Credentials(error.to_string()))?
            {
                SessionCredentials::Absent => {
                    record.sealed_bindings = None;
                    record.ephemeral_bindings = None;
                }
                SessionCredentials::Encrypted(blob) => {
                    record.sealed_bindings = Some(blob);
                    record.ephemeral_bindings = None;
                }
                SessionCredentials::Transient(bindings) => {
                    record.sealed_bindings = None;
                    record.ephemeral_bindings = Some(bindings);
                }
            }
        }
    }
    if matches!(session.status(), SessionStatus::Closed(_)) {
        record.sealed_bindings = None;
        record.ephemeral_bindings = None;
        record.mcp_state = None;
        record.cleanup_pending |= was_active;
    }
    Ok(record)
}

fn write_domain(record: &mut SessionRecord, session: &Session) {
    record.session_id = session.id().as_str().to_owned();
    record.blueprint_name = session.binding().blueprint().to_owned();
    record.variables = session.binding().variables().clone();
    record.status = match session.status() {
        SessionStatus::Active => RecordStatus::Active,
        SessionStatus::Closed(_) => RecordStatus::Closed,
    };
    // Existing SQL and legacy JSON represent an unknown cause with no value.
    record.closed_reason = session
        .closed_reason()
        .filter(|reason| *reason != ClosedReason::Unknown);
    record.idle_timeout = session.lifetime().idle_timeout();
    record.last_activity = session.lifetime().last_activity();
    if let Some(kind) = session.root().kind() {
        record.legacy_root_unresolved = false;
        record.root_vfs_type = kind;
        record.root_vfs_path = session.root().path().map(Path::to_path_buf);
    }
}

fn rule_error(error: SessionRuleError) -> StoreError {
    StoreError::Io(error.to_string())
}

#[async_trait::async_trait]
impl crate::application::sessions::ports::SessionRepository for SessionPersistence {
    async fn get(&self, id: &str) -> Result<Option<Session>, StoreError> {
        self.store
            .load(id)
            .await?
            .map(|record| restore_domain(&record, &self.session_root))
            .transpose()
    }
}
