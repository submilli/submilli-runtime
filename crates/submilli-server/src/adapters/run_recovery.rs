//! Bounded lifecycle queries; programs and results are loaded separately.
use super::run::{SELECT, decode};
use crate::application::sessions::recovery::{RunQuery, RunStatusFilter};
use crate::database::DatabaseError;
use crate::domain::run::Run;
use sqlx::SqliteConnection;

pub(crate) async fn list(
    connection: &mut SqliteConnection,
    query: RunQuery,
) -> Result<Vec<Run>, DatabaseError> {
    let mut sql = sqlx::QueryBuilder::new(SELECT);
    sql.push(" WHERE run_id>")
        .push_bind(query.after.map(|id| id.to_string()).unwrap_or_default());
    if query.recovery {
        sql.push(" AND recovery_mode='recorded' AND resources_retired=0");
    }
    if let Some(session) = query.session {
        sql.push(" AND session_id=").push_bind(session);
    }
    if let Some(status) = query.status {
        sql.push(" AND state=").push_bind(match status {
            RunStatusFilter::Queued => "reserved",
            RunStatusFilter::Running => "running",
            RunStatusFilter::Stuck => "blocked",
            RunStatusFilter::Cancelled => "cancelled",
            RunStatusFilter::Aborted => "aborted",
            RunStatusFilter::Completed => "completed",
            RunStatusFilter::Indeterminate => "indeterminate",
            RunStatusFilter::Expired => "expired",
        });
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

pub(crate) async fn resources_retired(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<bool, DatabaseError> {
    Ok(
        sqlx::query_scalar("SELECT resources_retired FROM runs WHERE run_id=?")
            .bind(id)
            .fetch_one(connection)
            .await?,
    )
}

pub(crate) async fn retire(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<(), DatabaseError> {
    let changed = sqlx::query("UPDATE runs SET resources_retired=1 WHERE run_id=? AND state IN ('completed','cancelled','aborted','expired')")
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
