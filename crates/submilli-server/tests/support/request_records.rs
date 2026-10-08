#![allow(dead_code)] // Shared fixture modules expose different subsets to each integration test.
//! SQL fixtures for tests that inspect or inject durable request evidence.
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use std::time::SystemTime;
use submilli_server::database::{DatabaseError, ServerDatabase};
use submilli_server::domain::idempotent_request::{RecordedOutcome, RequestState};

#[derive(Debug, PartialEq)]
pub struct Record {
    pub session_id: String,
    pub key: String,
    pub fingerprint: String,
    pub state: RequestState,
}
impl Record {
    #[allow(dead_code)] // The last-run integration assertions inspect the stored response.
    pub fn outcome(&self) -> Option<&RecordedOutcome> {
        match &self.state {
            RequestState::Completed(outcome) => Some(outcome),
            _ => None,
        }
    }

    pub fn indeterminate(session: &str, key: &str, fingerprint: String) -> Self {
        Self {
            session_id: session.into(),
            key: key.into(),
            fingerprint,
            state: RequestState::Indeterminate,
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
        assert_eq!(
            record.state,
            RequestState::Indeterminate,
            "fixture seeds abandoned requests only"
        );
        self.database.transaction(move |connection| Box::pin(async move {
            let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64;
            sqlx::query("INSERT INTO idempotent_requests(session_id,request_key,fingerprint,reservation_id,owner_generation,created_at_unix_ms,state) VALUES (?,?,?,?,?,?, 'indeterminate')")
                .bind(record.session_id).bind(record.key).bind(record.fingerprint).bind(uuid::Uuid::new_v4().to_string()).bind("previous-server").bind(time).execute(connection).await?;
            Ok(())
        })).await
    }
    pub async fn load(&self, session: &str, key: &str) -> Result<Option<Record>, DatabaseError> {
        let session = session.to_owned();
        let key = key.to_owned();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    let Some(row) = sqlx::query(
                        "SELECT * FROM idempotent_requests WHERE session_id=? AND request_key=?",
                    )
                    .bind(&session)
                    .bind(&key)
                    .fetch_optional(connection)
                    .await?
                    else {
                        return Ok(None);
                    };
                    let state = match row.try_get::<&str, _>("state")? {
                        "completed" => RequestState::Completed(RecordedOutcome {
                            status: row.try_get("response_status")?,
                            body: String::from_utf8(row.try_get("response_body")?).unwrap(),
                        }),
                        "reserved" => RequestState::Reserved,
                        _ => RequestState::Indeterminate,
                    };
                    Ok(Some(Record {
                        session_id: session,
                        key,
                        fingerprint: row.try_get("fingerprint")?,
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
                    Ok(
                        sqlx::query_scalar("SELECT DISTINCT session_id FROM idempotent_requests")
                            .fetch_all(connection)
                            .await?,
                    )
                })
            })
            .await
    }
}
