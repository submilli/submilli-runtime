//! Durable registrations and immutable executable closures in the session database.
use crate::adapters::sealing::Sealer;
use crate::application::error::StoreError;
use crate::application::sessions::finalization::Finalizations;
use crate::database::{DatabaseError, ServerDatabase};
use crate::domain::finalization::{
    Finalization, FinalizationStatus, FinalizerFailure, FinalizerInvocation,
};
use interpreter::runtime::session_finalizers::{FinalizerFuture, SessionFinalizers};
use interpreter::runtime::session_kv::{SessionKvError, SessionKvLimits};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct StoredFinalizers {
    pub database: Arc<ServerDatabase>,
    pub sealer: Sealer,
}

pub(crate) struct RegistrationProvider {
    pub store: StoredFinalizers,
    pub session: String,
    pub archives: BTreeMap<String, Arc<Vec<u8>>>,
    pub limits: SessionKvLimits,
}

impl SessionFinalizers for RegistrationProvider {
    fn artifact_bytes(&self, package: &str) -> u64 {
        self.archives
            .get(package)
            .map_or(0, |archive| archive.len() as u64)
    }
    fn register<'a>(
        &'a self,
        package: &'a str,
        key: &'a [u16],
        value: &'a [u16],
    ) -> FinalizerFuture<'a> {
        Box::pin(async move {
            self.limits.check_write(key, value).map_err(|mut error| {
                match &mut error {
                    SessionKvError::LimitExceeded { operation, .. }
                    | SessionKvError::InvalidKey { operation, .. } => {
                        *operation = "registerFinalizer";
                    }
                    _ => {}
                }
                error
            })?;
            let archive = self
                .archives
                .get(package)
                .ok_or_else(|| backend("package has no validated finalizer artifact"))?
                .clone();
            if archive.len() > MAX_ARCHIVE_BYTES {
                return Err(backend("finalizer artifact closure exceeds 64 MiB"));
            }
            let digest = format!("{:x}", Sha256::digest(archive.as_slice()));
            let id = uuid::Uuid::new_v4().to_string();
            let sealed = self.store.sealer.key_id().is_some();
            let plain = units_to_bytes(value);
            let payload = if sealed {
                self.store
                    .sealer
                    .seal(&owner(&id), &plain)
                    .map_err(|_| backend("cannot seal finalizer arguments"))?
            } else {
                plain
            };
            let session = self.session.clone();
            let package = package.to_owned();
            let key = units_to_bytes(key);
            let retained = (key.len() as u64)
                .saturating_add((value.len() as u64).saturating_mul(2))
                .saturating_add(64);
            let limits = self.limits;
            self.store.database.transaction(move |connection| Box::pin(async move {
                require_active(connection, &session).await?;
                let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? AND package=? AND local_key=? AND status IN ('registered','pending','running'))")
                    .bind(&session).bind(&package).bind(&key).fetch_one(&mut *connection).await?;
                if duplicate { return Err(DatabaseError::FinalizerRegistrationRejected("duplicate package finalizer key")); }
                let (count, bytes): (i64, i64) = sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(retained_bytes),0) FROM session_finalizers WHERE session_id=? AND status IN ('registered','pending','running')")
                    .bind(&session).fetch_one(&mut *connection).await?;
                let archive_bytes: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(length(archive)),0) FROM finalizer_artifacts WHERE digest IN (SELECT artifact_digest FROM session_finalizers WHERE session_id=?) AND digest!=?")
                    .bind(&session).bind(&digest).fetch_one(&mut *connection).await?;
                let total = (bytes as u64).saturating_add(retained).saturating_add(archive_bytes as u64).saturating_add(archive.len() as u64);
                if count as u64 >= limits.max_entries || total > limits.max_session_bytes {
                    return Err(DatabaseError::FinalizerRegistrationRejected("finalizer registration quota exceeded"));
                }
                sqlx::query("INSERT OR IGNORE INTO finalizer_artifacts(digest,archive) VALUES (?,?)")
                    .bind(&digest).bind(archive.as_slice()).execute(&mut *connection).await?;
                sqlx::query("INSERT INTO session_finalizers(id,session_id,package,local_key,artifact_digest,payload,sealed,retained_bytes,status) VALUES (?,?,?,?,?,?,?,?,'registered')")
                    .bind(id).bind(session).bind(package).bind(key).bind(digest).bind(payload).bind(sealed).bind(i64::try_from(retained).map_err(|_| DatabaseError::Import("finalizer size overflow".into()))?)
                    .execute(connection).await?;
                Ok(())
            })).await.map_err(registration_error)
        })
    }
    fn unregister<'a>(&'a self, package: &'a str, key: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async move {
            self.limits.check_key("unregisterFinalizer", key)?;
            let session = self.session.clone();
            let package = package.to_owned();
            let key = units_to_bytes(key);
            self.store.database.transaction(move |connection| Box::pin(async move {
                require_active(connection, &session).await?;
                let digest: Option<String> = sqlx::query_scalar("DELETE FROM session_finalizers WHERE session_id=? AND package=? AND local_key=? AND status='registered' RETURNING artifact_digest")
                    .bind(session).bind(package).bind(key).fetch_optional(&mut *connection).await?;
                collect_artifact(connection, digest.as_deref()).await
            })).await.map_err(registration_error)
        })
    }
}

#[async_trait::async_trait]
impl Finalizations for StoredFinalizers {
    async fn claim_next(&self, session: &str) -> Result<Option<FinalizerInvocation>, StoreError> {
        let session = session.to_owned();
        let sealer = self.sealer.clone();
        let generation = self.database.generation().map_err(StoreError::from)?;
        self.database.transaction(move |connection| Box::pin(async move {
            let row = sqlx::query("SELECT id,package,local_key,payload,sealed,artifact_digest FROM session_finalizers WHERE session_id=? AND status='pending' ORDER BY created_at,id LIMIT 1")
                .bind(&session).fetch_optional(&mut *connection).await?;
            let Some(row) = row else { return Ok(None); };
            let id: String = row.try_get("id")?;
            let payload: Vec<u8> = row.try_get("payload")?;
            let plain = if row.try_get::<bool,_>("sealed")? {
                sealer.open(&owner(&id), &payload, "finalizer arguments").map_err(|_| FinalizerFailure::AuthorityUnavailable)
            } else { Ok(payload) };
            let value = plain.and_then(|plain| bytes_to_units(&plain).map_err(|_| FinalizerFailure::ArtifactUnavailable));
            let mut lifecycle = Finalization::restore(FinalizationStatus::Pending);
            if !lifecycle.claim() { return Err(DatabaseError::SessionConflict); }
            let invocation = FinalizerInvocation {
                id: id.clone(), session_id: session,
                package: row.try_get("package")?, key: bytes_to_units(&row.try_get::<Vec<u8>,_>("local_key")?)?,
                value, artifact_digest: row.try_get("artifact_digest")?,
            };
            sqlx::query("UPDATE session_finalizers SET status=?,started_at=CURRENT_TIMESTAMP,attempt_owner=? WHERE id=? AND status='pending'")
                .bind(status_name(lifecycle.status())).bind(generation).bind(id).execute(connection).await?;
            Ok(Some(invocation))
        })).await.map_err(StoreError::from)
    }
    async fn complete(
        &self,
        id: &str,
        outcome: Result<(), FinalizerFailure>,
    ) -> Result<(), StoreError> {
        let id = id.to_owned();
        self.database.transaction(move |connection| Box::pin(async move {
            let mut lifecycle = Finalization::restore(FinalizationStatus::Running);
            if !lifecycle.complete(outcome) { return Err(DatabaseError::SessionConflict); }
            let digest: Option<String> = sqlx::query_scalar("SELECT artifact_digest FROM session_finalizers WHERE id=? AND status='running'")
                .bind(&id).fetch_optional(&mut *connection).await?;
            sqlx::query("UPDATE session_finalizers SET status=?,failure=?,completed_at=CURRENT_TIMESTAMP,payload=NULL,artifact_digest=NULL WHERE id=? AND status='running'")
                .bind(status_name(lifecycle.status())).bind(lifecycle.failure().map(FinalizerFailure::as_str)).bind(id)
                .execute(&mut *connection).await?;
            collect_artifact(connection, digest.as_deref()).await
        })).await.map_err(StoreError::from)
    }
}
impl StoredFinalizers {
    pub async fn recover(&self) -> Result<(), StoreError> {
        let generation = self.database.generation().map_err(StoreError::from)?;
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    let mut lifecycle = Finalization::restore(FinalizationStatus::Running);
                    if lifecycle.recover() {
                        sqlx::query(
                            "UPDATE session_finalizers SET status=?,attempt_owner=NULL WHERE status='running' AND (attempt_owner IS NULL OR attempt_owner!=?)",
                        )
                        .bind(status_name(lifecycle.status()))
                        .bind(generation)
                        .execute(connection)
                        .await?;
                    }
                    Ok(())
                })
            })
            .await
            .map_err(StoreError::from)
    }
    pub async fn archive(&self, digest: &str) -> Result<Vec<u8>, DatabaseError> {
        let digest = digest.to_owned();
        self.database
            .read(move |connection| {
                Box::pin(async move {
                    let archive: Vec<u8> = sqlx::query_scalar(
                        "SELECT archive FROM finalizer_artifacts WHERE digest=?",
                    )
                    .bind(&digest)
                    .fetch_one(connection)
                    .await?;
                    if archive.len() > MAX_ARCHIVE_BYTES
                        || format!("{:x}", Sha256::digest(&archive)) != digest
                    {
                        return Err(DatabaseError::Import(
                            "finalizer artifact digest mismatch".into(),
                        ));
                    }
                    Ok(archive)
                })
            })
            .await
    }
}
async fn require_active(
    connection: &mut sqlx::SqliteConnection,
    session: &str,
) -> Result<(), DatabaseError> {
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=? AND status='active')",
    )
    .bind(session)
    .fetch_one(connection)
    .await?;
    if active {
        Ok(())
    } else {
        Err(DatabaseError::FinalizerRegistrationRejected(
            "session is closed",
        ))
    }
}
async fn collect_artifact(
    connection: &mut sqlx::SqliteConnection,
    digest: Option<&str>,
) -> Result<(), DatabaseError> {
    if let Some(digest) = digest {
        sqlx::query("DELETE FROM finalizer_artifacts WHERE digest=? AND NOT EXISTS(SELECT 1 FROM session_finalizers WHERE artifact_digest=?)")
            .bind(digest).bind(digest).execute(connection).await?;
    }
    Ok(())
}
fn owner(id: &str) -> String {
    format!("submilli:finalizer:v1:{id}")
}
fn backend(message: &str) -> SessionKvError {
    SessionKvError::Backend {
        operation: "finalizer",
        message: message.to_owned(),
    }
}
fn units_to_bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_be_bytes()).collect()
}
fn bytes_to_units(bytes: &[u8]) -> Result<Vec<u16>, DatabaseError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DatabaseError::Import(
            "corrupt finalizer UTF-16 payload".into(),
        ));
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect())
}

#[derive(Debug, serde::Serialize)]
pub struct FinalizerDiagnostic {
    pub package: String,
    pub key: String,
    pub status: String,
    pub failure: Option<String>,
}
impl StoredFinalizers {
    pub async fn diagnostics(&self, session: &str) -> Result<Vec<FinalizerDiagnostic>, StoreError> {
        let session = session.to_owned();
        self.database.read(move |connection| Box::pin(async move {
            let rows = sqlx::query("SELECT package,local_key,status,failure FROM session_finalizers WHERE session_id=? ORDER BY created_at,id")
                .bind(session).fetch_all(connection).await?;
            rows.into_iter().map(|row| Ok(FinalizerDiagnostic {
                package: row.try_get("package")?,
                key: String::from_utf16_lossy(&bytes_to_units(&row.try_get::<Vec<u8>,_>("local_key")?)?),
                status: row.try_get("status")?, failure: row.try_get("failure")?,
            })).collect()
        })).await.map_err(Into::into)
    }
    pub async fn has_unfinished(&self, session: &str) -> Result<bool, StoreError> {
        let session = session.to_owned();
        self.database.read(move |connection| Box::pin(async move {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? AND status IN ('registered','pending','running'))")
                .bind(session).fetch_one(connection).await.map_err(Into::into)
        })).await.map_err(Into::into)
    }
}

fn registration_error(error: DatabaseError) -> SessionKvError {
    match error {
        DatabaseError::FinalizerRegistrationRejected(reason) => SessionKvError::InvalidKey {
            operation: "finalizer",
            key: String::new(),
            reason,
        },
        other => backend(&other.to_string()),
    }
}

fn status_name(status: FinalizationStatus) -> &'static str {
    match status {
        FinalizationStatus::Registered => "registered",
        FinalizationStatus::Pending => "pending",
        FinalizationStatus::Running => "running",
        FinalizationStatus::Succeeded => "succeeded",
        FinalizationStatus::Failed => "failed",
    }
}

/// Called in the same transaction that persists a session's closing transition.
/// Preserve credentials from the active revision before its normal bindings clear.
pub(crate) async fn terminate(
    connection: &mut sqlx::SqliteConnection,
    session: &str,
) -> Result<(), DatabaseError> {
    let mut lifecycle = Finalization::restore(FinalizationStatus::Registered);
    if !lifecycle.terminate() {
        return Err(DatabaseError::SessionConflict);
    }
    sqlx::query("INSERT OR IGNORE INTO finalizer_context(session_id, encrypted_harness_bindings) SELECT session_id,encrypted_harness_bindings FROM sessions WHERE session_id=? AND status='active' AND EXISTS(SELECT 1 FROM session_finalizers WHERE session_id=? AND status='registered')")
        .bind(session).bind(session).execute(&mut *connection).await?;
    sqlx::query(
        "UPDATE session_finalizers SET status=? WHERE session_id=? AND status='registered'",
    )
    .bind(status_name(lifecycle.status()))
    .bind(session)
    .execute(connection)
    .await?;
    Ok(())
}
