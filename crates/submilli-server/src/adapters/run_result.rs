//! Exact retained HTTP responses. Lifecycle and result retention are independent.
use super::sealing::Sealer;
use crate::database::DatabaseError;
use crate::domain::idempotent_request::RecordedOutcome;
use sqlx::{Row, SqliteConnection};

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
) -> Result<Option<RecordedOutcome>, DatabaseError> {
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
    Ok(Some(RecordedOutcome {
        status: row.try_get("status")?,
        body: String::from_utf8(plain).map_err(invalid)?,
    }))
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
    result: &RecordedOutcome,
) -> Result<(), DatabaseError> {
    // Keyless non-durable deployments retain their existing plaintext behavior.
    let encrypted = sealer.key_id().is_some();
    let content = if encrypted {
        sealer
            .seal(&format!("run-result:v1:{id}"), result.body.as_bytes())
            .map_err(invalid)?
    } else {
        result.body.as_bytes().to_vec()
    };
    sqlx::query("INSERT INTO run_results(run_id,status,content,encrypted) VALUES (?,?,?,?)")
        .bind(id)
        .bind(result.status)
        .bind(content)
        .bind(encrypted)
        .execute(connection)
        .await?;
    Ok(())
}

fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(format!("invalid run result: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use submilli_shared::secret_store::SecretCipher;

    #[tokio::test]
    async fn results_are_sealed_and_bound_to_their_run_without_hiding_metadata() {
        let database = crate::database::ServerDatabase::open_ephemeral()
            .await
            .unwrap();
        database.transaction(|connection| Box::pin(async move {
            sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state,completed_at_unix_ms) VALUES('one','one','s','fp','owner',1,0,'completed',1),('two','two','s','fp','owner',2,0,'completed',1)")
                .execute(&mut *connection).await?;
            let sealer = Sealer::new(Some(Arc::new(SecretCipher::from_key([9; 32]))));
            let result = RecordedOutcome { status: 201, body: "secret result\n".into() };
            save(connection, &sealer, "one", &result).await?;
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
