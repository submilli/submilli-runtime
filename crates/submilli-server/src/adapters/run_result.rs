//! Run results use the existing response envelope on disk, preserving HTTP retries.
use super::sealing::Sealer;
use crate::application::error::StoreError;
use crate::database::DatabaseError;
use crate::database::ServerDatabase;
use crate::domain::run::Run;
use crate::domain::run_result::{RunOutput, RunResult};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};
use std::sync::Arc;
use submilli_shared::secret_store::SecretCipher;

/// A transport snapshot, kept outside the domain. Legacy responses are returned verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunResponse {
    pub status: u16,
    pub body: String,
}

pub(crate) struct RunResponses {
    database: Arc<ServerDatabase>,
    sealer: Sealer,
}

/// Metadata and its retained response from the same database snapshot.
pub(crate) struct ObservedRun {
    pub run: Run,
    pub response: Option<RunResponse>,
}

impl RunResponses {
    pub fn new(database: Arc<ServerDatabase>, cipher: Option<Arc<SecretCipher>>) -> Self {
        Self {
            database,
            sealer: Sealer::new(cipher),
        }
    }

    pub async fn get(&self, id: &str) -> Result<Option<RunResponse>, StoreError> {
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        Ok(self
            .database
            .transaction(move |connection| {
                Box::pin(async move { response(connection, &sealer, &id).await })
            })
            .await?)
    }
    pub async fn observe(&self, id: &str) -> Result<Option<ObservedRun>, StoreError> {
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        Ok(self
            .database
            .transaction(move |connection| {
                Box::pin(async move {
                    let Some(run) = super::run::get_run(connection, &id).await? else {
                        return Ok(None);
                    };
                    let response = response(connection, &sealer, &id).await?;
                    Ok(Some(ObservedRun { run, response }))
                })
            })
            .await?)
    }
}

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
) -> Result<Option<RunResult>, DatabaseError> {
    let Some(response) = response(connection, sealer, id).await? else {
        return Ok(None);
    };
    let stored: StoredRunResult = serde_json::from_str(&response.body).map_err(invalid)?;
    Ok(Some(RunResult::new(id.into(), stored.output)))
}

async fn response(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
) -> Result<Option<RunResponse>, DatabaseError> {
    let Some(row) = sqlx::query("SELECT status,content,encrypted FROM run_results WHERE run_id=?")
        .bind(id)
        .fetch_optional(connection)
        .await?
    else {
        return Ok(None);
    };
    let content: Vec<u8> = row.try_get("content")?;
    let plain = if row.try_get::<bool, _>("encrypted")? {
        sealer
            .open(&format!("run-result:v1:{id}"), &content, "run result")
            .map_err(invalid)?
    } else {
        content
    };
    Ok(Some(RunResponse {
        status: row.try_get("status")?,
        body: String::from_utf8(plain).map_err(invalid)?,
    }))
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    result: &RunResult,
) -> Result<(), DatabaseError> {
    let id = result.run_id().as_str();
    let row = sqlx::query("SELECT execution_id,session_id FROM runs WHERE run_id=?")
        .bind(id)
        .fetch_one(&mut *connection)
        .await?;
    let stored = StoredRunResult {
        status: "finished".into(),
        execution_id: row
            .try_get::<Option<String>, _>("execution_id")?
            .unwrap_or_else(|| id.to_owned()),
        session_id: row.try_get("session_id")?,
        output: result.output().clone(),
    };
    let body = serde_json::to_vec(&stored).map_err(invalid)?;
    // Keyless non-durable deployments retain their existing plaintext behavior.
    let encrypted = sealer.key_id().is_some();
    let content = if encrypted {
        sealer
            .seal(&format!("run-result:v1:{id}"), &body)
            .map_err(invalid)?
    } else {
        body
    };
    sqlx::query("INSERT INTO run_results(run_id,status,content,encrypted) VALUES (?,?,?,?)")
        .bind(id)
        .bind(200)
        .bind(content)
        .bind(encrypted)
        .execute(connection)
        .await?;
    Ok(())
}

// Field order matches the finished HTTP response. Read retries use its saved bytes,
// so future changes to this codec cannot change responses already delivered.
#[derive(Serialize, Deserialize)]
struct StoredRunResult {
    status: String,
    execution_id: String,
    session_id: String,
    #[serde(flatten)]
    output: RunOutput,
}

fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(format!("invalid run result: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::execution_error::{
        DenialDetails, DiagnosticNote, DiagnosticPayload, ErrorKind, ExecuteError,
    };
    use crate::handlers::execute::ExecuteResponse;
    use std::sync::Arc;
    use submilli_shared::secret_store::SecretCipher;

    #[tokio::test]
    async fn result_codec_preserves_outputs_and_the_original_http_bytes() {
        let database = crate::database::ServerDatabase::open_ephemeral()
            .await
            .unwrap();
        let outputs = [
            RunOutput {
                result: Some("a result\n\"with escapes\" and 🦀".into()),
                ..Default::default()
            },
            RunOutput {
                console: vec!["before failure".into()],
                error: Some(ExecuteError {
                    kind: ErrorKind::PermissionDenied,
                    message: "permission denied".into(),
                    diagnostics: vec![DiagnosticPayload {
                        severity: "error".into(),
                        line: 1,
                        column: 2,
                        message: "denied here".into(),
                        notes: vec![DiagnosticNote {
                            line: 3,
                            column: 4,
                            message: "caller".into(),
                        }],
                    }],
                    denial: Some(DenialDetails {
                        caller: "@test/package".into(),
                        capability: "fs.write".into(),
                        source: "policy".into(),
                    }),
                }),
                discovery_warnings: vec!["unsupported tool".into()],
                ..Default::default()
            },
            RunOutput {
                error: Some(ExecuteError {
                    kind: ErrorKind::RuntimeError,
                    message: "failed".into(),
                    diagnostics: vec![],
                    denial: None,
                }),
                ..Default::default()
            },
        ];
        for output in outputs {
            database.transaction(move |connection| Box::pin(async move {
                sqlx::query("DELETE FROM run_results").execute(&mut *connection).await?;
                sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,execution_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state,completed_at_unix_ms) VALUES('run','run','execution','session','fp','owner',1,0,'completed',1)")
                    .execute(&mut *connection).await?;
                let original = crate::handlers::runs::finished_body(&ExecuteResponse {
                    execution_id: "execution".into(), session_id: "session".into(),
                    result: output.result.clone(), console: output.console.clone(), error: output.error.clone(),
                    discovery_warnings: output.discovery_warnings.clone(),
                });
                assert!(original.recordable);
                let result = RunResult::new("run".into(), output);
                let sealer = Sealer::default();
                save(connection, &sealer, &result).await?;
                assert_eq!(get(connection, &sealer, "run").await?, Some(result));
                let saved = response(connection, &sealer, "run").await?.unwrap();
                assert_eq!(saved.status, 200);
                assert_eq!(saved.body, original.body);
                Ok(())
            })).await.unwrap();
        }
    }

    #[tokio::test]
    async fn legacy_responses_keep_their_status_and_exact_bytes_without_decoding() {
        let database = Arc::new(
            crate::database::ServerDatabase::open_ephemeral()
                .await
                .unwrap(),
        );
        database.transaction(|connection| Box::pin(async move {
            sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state,completed_at_unix_ms) VALUES('legacy','legacy','session','fp','owner',1,0,'completed',1)")
                .execute(&mut *connection).await?;
            sqlx::query("INSERT INTO run_results(run_id,status,content,encrypted) VALUES ('legacy',201,?,0)")
                .bind(b"exact legacy bytes\n".to_vec()).execute(connection).await?;
            Ok(())
        })).await.unwrap();
        let responses = RunResponses::new(database.clone(), None);
        let response = responses.get("legacy").await.unwrap().unwrap();
        assert_eq!(response.status, 201);
        assert_eq!(response.body, "exact legacy bytes\n");
        let observed = responses.observe("legacy").await.unwrap().unwrap();
        assert_eq!(
            observed.run.status(),
            crate::domain::run_status::RunStatus::Completed
        );
        assert_eq!(observed.response, Some(response));
        assert!(responses.observe("missing").await.unwrap().is_none());

        database
            .transaction(|connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM run_results WHERE run_id='legacy'")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("UPDATE runs SET state='expired' WHERE run_id='legacy'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        let expired = responses.observe("legacy").await.unwrap().unwrap();
        assert_eq!(
            expired.run.status(),
            crate::domain::run_status::RunStatus::Expired
        );
        assert!(expired.response.is_none());
    }

    #[tokio::test]
    async fn results_are_sealed_and_bound_to_their_run_without_hiding_metadata() {
        let database = crate::database::ServerDatabase::open_ephemeral()
            .await
            .unwrap();
        database.transaction(|connection| Box::pin(async move {
            sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state,completed_at_unix_ms) VALUES('one','one','s','fp','owner',1,0,'completed',1),('two','two','s','fp','owner',2,0,'completed',1)")
                .execute(&mut *connection).await?;
            let sealer = Sealer::new(Some(Arc::new(SecretCipher::from_key([9; 32]))));
            let result = RunResult::new("one".into(), RunOutput { result: Some("secret result\n".into()), ..Default::default() });
            save(connection, &sealer, &result).await?;
            assert_eq!(get(connection, &sealer, "one").await?, Some(result));
            let content: Vec<u8> = sqlx::query_scalar("SELECT content FROM run_results WHERE run_id='one'").fetch_one(&mut *connection).await?;
            assert!(!content.windows(6).any(|bytes| bytes == b"secret"));
            assert!(get(connection, &Sealer::default(), "one").await.is_err());
            sqlx::query("INSERT INTO run_results SELECT 'two',status,content,encrypted FROM run_results WHERE run_id='one'").execute(&mut *connection).await?;
            assert!(get(connection, &sealer, "two").await.is_err());
            // Corrupt/unreadable payloads must not prevent lifecycle queries.
            let run = crate::adapters::run::get_run(connection, "two").await?.unwrap();
            assert_eq!(run.status(), crate::domain::run_status::RunStatus::Completed);
            Ok(())
        })).await.unwrap();
    }
}
