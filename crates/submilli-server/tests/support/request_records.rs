#![allow(dead_code)] // Shared fixture modules expose different subsets to each integration test.
//! SQL fixtures for tests that inspect or inject durable request evidence.
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use std::time::SystemTime;
use submilli_server::database::{DatabaseError, ServerDatabase};
use submilli_server::domain::idempotent_request::RunStatus;

#[derive(Debug, PartialEq)]
pub struct RecordedResponse {
    pub status: u16,
    pub body: String,
}

use submilli_server::domain::run_status::{IndeterminateReason, StuckReason};

#[derive(Debug, PartialEq)]
pub struct Record {
    pub session_id: String,
    pub key: String,
    pub fingerprint: String,
    pub state: RunStatus,
    pub result: Option<RecordedResponse>,
}
impl Record {
    #[allow(dead_code)] // The last-run integration assertions inspect the stored response.
    pub fn outcome(&self) -> Option<&RecordedResponse> {
        self.result.as_ref()
    }

    pub fn indeterminate(session: &str, key: &str, fingerprint: String) -> Self {
        Self {
            session_id: session.into(),
            key: key.into(),
            fingerprint,
            state: RunStatus::Indeterminate(IndeterminateReason::Interrupted),
            result: None,
        }
    }

    #[allow(dead_code)] // Used by the response-expiry integration test.
    pub fn expired(session: &str, key: &str, fingerprint: String) -> Self {
        Self {
            session_id: session.into(),
            key: key.into(),
            fingerprint,
            state: RunStatus::Expired,
            result: None,
        }
    }
}
pub fn code_fingerprint(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}
pub struct RequestRecords {
    pub database: Arc<ServerDatabase>,
}
impl RequestRecords {
    pub fn with_database(database: Arc<ServerDatabase>) -> Self {
        Self { database }
    }
    pub fn ephemeral() -> Self {
        Self::with_database(Arc::new(
            futures::executor::block_on(ServerDatabase::open_ephemeral()).unwrap(),
        ))
    }
    #[allow(dead_code)] // Shared fixture; only file-backed integration tests use this constructor.
    pub fn file(path: &std::path::Path) -> Self {
        Self::with_database(Arc::new(
            futures::executor::block_on(ServerDatabase::open(path)).unwrap(),
        ))
    }
    pub async fn put(&self, record: Record) -> Result<(), DatabaseError> {
        let (state, completed, owner_generation) = match record.state {
            RunStatus::Indeterminate(_) => ("indeterminate", false, "previous-server"),
            RunStatus::Expired => ("expired", true, "this-server"),
            _ => panic!("fixture seeds abandoned or expired requests only"),
        };
        self.database.transaction(move |connection| Box::pin(async move {
            let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64;
            let completed = completed.then_some(time);
            let identity = uuid::Uuid::new_v4().to_string();
            let order: i64 = sqlx::query_scalar("UPDATE sessions SET last_run_order=last_run_order+1 WHERE session_id=? AND last_run_order<9223372036854775807 RETURNING last_run_order").bind(&record.session_id).fetch_one(&mut *connection).await?;
            sqlx::query("INSERT INTO runs(run_id,admission_order,session_id,request_key,fingerprint,reservation_id,owner_generation,created_at_unix_ms,state,completed_at_unix_ms) VALUES (?,?,?,?,?,?,?,?,?,?)")
                .bind(&identity).bind(order)
                .bind(record.session_id).bind(record.key).bind(record.fingerprint).bind(identity).bind(owner_generation).bind(time).bind(state).bind(completed).execute(connection).await?;
            Ok(())
        })).await
    }
    pub async fn load(&self, session: &str, key: &str) -> Result<Option<Record>, DatabaseError> {
        let session = session.to_owned();
        let key = key.to_owned();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    let Some(row) =
                        sqlx::query("SELECT runs.*,run_results.status AS response_status,run_results.content AS response_body FROM runs LEFT JOIN run_results USING(run_id) WHERE session_id=? AND request_key=?")
                            .bind(&session)
                            .bind(&key)
                            .fetch_optional(connection)
                            .await?
                    else {
                        return Ok(None);
                    };
                    let state = match row.try_get::<&str, _>("state")? {
                        "completed" => RunStatus::Completed,
                        "reserved" => RunStatus::Queued,
                        "expired" => RunStatus::Expired,
                        "running" => RunStatus::Running,
                        "blocked" => RunStatus::Stuck(match row.try_get::<&str, _>("reason")? {
                            "credentials_unavailable" => StuckReason::Credentials,
                            "storage_unavailable" => StuckReason::Storage,
                            "incompatible_environment" => StuckReason::Compatibility,
                            "invalid_history" => StuckReason::History,
                            "fuel_exhausted" => StuckReason::Fuel,
                            "execution_timeout" => StuckReason::Timeout,
                            "execution_interrupted" => StuckReason::Interrupted,
                            "durability_disabled" => StuckReason::Disabled,
                            "replay_unavailable" => StuckReason::ReplayUnavailable,
                            _ => return Err(DatabaseError::Import("invalid stuck reason".into())),
                        }),
                        "cancelled" => RunStatus::Cancelled,
                        "aborted" => RunStatus::Aborted,
                        "finished" => RunStatus::Completed,
                        "indeterminate" => RunStatus::Indeterminate(IndeterminateReason::Interrupted),
                        state => {
                            return Err(DatabaseError::Import(format!(
                                "invalid run state: {state}"
                            )));
                        }
                    };
                    Ok(Some(Record {
                        session_id: session,
                        key,
                        fingerprint: row.try_get("fingerprint")?,
                        result: row.try_get::<Option<u16>,_>("response_status")?.map(|status| RecordedResponse {
                            status, body: String::from_utf8(row.try_get("response_body").unwrap()).unwrap(),
                        }),
                        state,
                    }))
                })
            })
            .await
    }
    #[allow(dead_code)] // Used by refusal integration tests.
    pub async fn session_ids(&self) -> Result<Vec<String>, DatabaseError> {
        self.database
            .transaction(|connection| {
                Box::pin(async move {
                    Ok(sqlx::query_scalar("SELECT DISTINCT session_id FROM runs")
                        .fetch_all(connection)
                        .await?)
                })
            })
            .await
    }
}
