//! Bounded lifecycle queries deliberately do not open program/history payloads.
use crate::application::sessions::recovery::{RunQuery, RunStatus, RunSummary};
use crate::database::DatabaseError;
use crate::domain::session::SessionId;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, UNIX_EPOCH};

const SUMMARY: &str = "SELECT run_id,session_id,fingerprint,state,reason,recovery_mode,revision,attempt,owner_generation,admission_order,created_at_unix_ms,completed_at_unix_ms FROM runs";

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<RunSummary>, DatabaseError> {
    let mut query = sqlx::QueryBuilder::new(SUMMARY);
    query.push(" WHERE run_id=").push_bind(id);
    query
        .build()
        .fetch_optional(connection)
        .await?
        .map(decode)
        .transpose()
}

pub(crate) async fn list(
    connection: &mut SqliteConnection,
    query: RunQuery,
) -> Result<Vec<RunSummary>, DatabaseError> {
    let mut sql = sqlx::QueryBuilder::new(SUMMARY);
    sql.push(" WHERE run_id>")
        .push_bind(query.after.map(|id| id.to_string()).unwrap_or_default());
    if query.recovery {
        sql.push(" AND recovery_mode='recorded' AND resources_retired=0");
    }
    if let Some(session) = query.session {
        sql.push(" AND session_id=").push_bind(session);
    }
    if let Some(status) = query.status {
        if status == RunStatus::Completed {
            sql.push(" AND state IN ('completed','finished')");
        } else {
            sql.push(" AND state=").push_bind(match status {
                RunStatus::Queued => "reserved",
                RunStatus::Stuck => "blocked",
                other => other.as_str(),
            });
        }
    }
    sql.push(" ORDER BY run_id LIMIT ")
        .push_bind(query.limit.clamp(1, 100) as i64);
    sql.build()
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(decode)
        .collect()
}

pub(crate) async fn retained(
    connection: &mut SqliteConnection,
    session: &str,
) -> Result<bool, DatabaseError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE session_id=? AND recovery_mode='recorded' AND state IN ('reserved','running','blocked'))")
        .bind(session).fetch_one(connection).await?)
}

pub(crate) async fn predecessor(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<bool, DatabaseError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs previous JOIN runs current ON previous.session_id=current.session_id WHERE current.run_id=? AND previous.admission_order<current.admission_order AND previous.state IN ('reserved','running','blocked','indeterminate'))")
        .bind(id).fetch_one(connection).await?)
}

pub(crate) async fn save_lifecycle(
    connection: &mut SqliteConnection,
    run: RunSummary,
) -> Result<(), DatabaseError> {
    let state = match run.status {
        RunStatus::Queued => "reserved",
        RunStatus::Stuck => "blocked",
        other => other.as_str(),
    };
    let previous = run
        .revision
        .checked_sub(1)
        .ok_or(DatabaseError::SessionConflict)?;
    let changed = sqlx::query(
        "UPDATE runs SET state=?,reason=?,attempt=?,revision=? WHERE run_id=? AND revision=?",
    )
    .bind(state)
    .bind(run.reason)
    .bind(run.attempt)
    .bind(run.revision)
    .bind(run.id.to_string())
    .bind(previous)
    .execute(connection)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

fn decode(row: SqliteRow) -> Result<RunSummary, DatabaseError> {
    let status = match row.try_get::<&str, _>("state")? {
        "reserved" => RunStatus::Queued,
        "running" => RunStatus::Running,
        "blocked" => RunStatus::Stuck,
        "cancelled" => RunStatus::Cancelled,
        "aborted" => RunStatus::Aborted,
        "completed" | "finished" => RunStatus::Completed,
        "indeterminate" => RunStatus::Indeterminate,
        "expired" => RunStatus::Expired,
        _ => return Err(DatabaseError::Import("invalid run state".into())),
    };
    let time = |value: i64| {
        let millis = u64::try_from(value)
            .map_err(|_| DatabaseError::Import("invalid run timestamp".into()))?;
        UNIX_EPOCH
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| DatabaseError::Import("invalid run timestamp".into()))
    };
    Ok(RunSummary {
        id: row.try_get::<String, _>("run_id")?.into(),
        session_id: SessionId::parse(row.try_get("session_id")?)
            .map_err(|error| DatabaseError::Import(error.to_string()))?,
        fingerprint: row.try_get("fingerprint")?,
        status,
        reason: row.try_get("reason")?,
        recorded: row.try_get::<&str, _>("recovery_mode")? == "recorded",
        revision: row.try_get("revision")?,
        attempt: row.try_get("attempt")?,
        owner: row.try_get("owner_generation")?,
        admission_order: row.try_get("admission_order")?,
        created_at: time(row.try_get("created_at_unix_ms")?)?,
        completed_at: row
            .try_get::<Option<i64>, _>("completed_at_unix_ms")?
            .map(time)
            .transpose()?,
    })
}

pub(crate) async fn result(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<crate::domain::idempotent_request::RecordedOutcome>, DatabaseError> {
    sqlx::query(
        "SELECT response_status,response_body FROM runs WHERE run_id=? AND state='completed'",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .map(|row| {
        Ok(crate::domain::idempotent_request::RecordedOutcome {
            status: row.try_get("response_status")?,
            body: String::from_utf8(row.try_get("response_body")?)
                .map_err(|e| DatabaseError::Import(e.to_string()))?,
        })
    })
    .transpose()
}

pub(crate) async fn retire(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<(), DatabaseError> {
    let changed = sqlx::query("UPDATE runs SET resources_retired=1 WHERE run_id=? AND state IN ('completed','finished','cancelled','aborted','expired')")
        .bind(id).execute(&mut *connection).await?;
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::SessionConflict);
    }
    sqlx::query("DELETE FROM durable_filesystem_chunks WHERE scope_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM durable_filesystem_writers WHERE scope_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM durable_filesystem_readers WHERE scope_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM durable_filesystem_listings WHERE scope_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM durable_filesystem_operations WHERE scope_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM durable_outcomes WHERE run_id=?")
        .bind(id)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn for_key(
    connection: &mut SqliteConnection,
    session: &str,
    key: &str,
) -> Result<Option<RunSummary>, DatabaseError> {
    let mut query = sqlx::QueryBuilder::new(SUMMARY);
    query
        .push(" WHERE session_id=")
        .push_bind(session)
        .push(" AND request_key=")
        .push_bind(key);
    query
        .build()
        .fetch_optional(connection)
        .await?
        .map(decode)
        .transpose()
}
