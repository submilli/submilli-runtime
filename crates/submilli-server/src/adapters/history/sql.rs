//! Publication ownership is committed with history; retirement queues deletion.
use crate::adapters::sql::database;
use crate::adapters::unit_of_work::execution_history;
use crate::application::error::StoreError;
use interpreter::runtime::durable::RunId;
use sqlx::SqliteConnection;
use submilli_durable::history::ExecutionAttempt;

pub(super) async fn initialize_store_identity(
    connection: &mut SqliteConnection,
    candidate: String,
) -> Result<(String, bool), crate::database::DatabaseError> {
    sqlx::query("INSERT OR IGNORE INTO history_object_store(singleton,identity) VALUES(1,?)")
        .bind(candidate)
        .execute(&mut *connection)
        .await?;
    let identity =
        sqlx::query_scalar("SELECT identity FROM history_object_store WHERE singleton=1")
            .fetch_one(&mut *connection)
            .await?;
    let has_objects = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history_objects)")
        .fetch_one(connection)
        .await?;
    Ok((identity, has_objects))
}

pub(super) async fn begin_initialization(
    connection: &mut SqliteConnection,
    identity: String,
    parents: String,
) -> Result<String, crate::database::DatabaseError> {
    sqlx::query("INSERT OR IGNORE INTO history_object_store(singleton,identity,pending_directory_sync) VALUES(1,?,?)")
        .bind(identity).bind(&parents).execute(&mut *connection).await?;
    sqlx::query("UPDATE history_object_store SET pending_directory_sync=(SELECT json_group_array(value) FROM (SELECT value FROM json_each(history_object_store.pending_directory_sync) UNION SELECT value FROM json_each(?))) WHERE singleton=1")
        .bind(parents).execute(&mut *connection).await?;
    Ok(sqlx::query_scalar(
        "SELECT pending_directory_sync FROM history_object_store WHERE singleton=1",
    )
    .fetch_one(connection)
    .await?)
}

pub(super) async fn finish_initialization(
    connection: &mut SqliteConnection,
) -> Result<(), crate::database::DatabaseError> {
    sqlx::query("UPDATE history_object_store SET pending_directory_sync='[]' WHERE singleton=1")
        .execute(connection)
        .await?;
    Ok(())
}

pub(super) async fn reserve(
    connection: &mut SqliteConnection,
    attempt: ExecutionAttempt,
    sequence: u64,
    id: String,
) -> Result<(), StoreError> {
    if execution_history::progress(connection, &attempt)
        .await?
        .position
        != sequence
    {
        return Err(StoreError::Conflict);
    }
    sqlx::query(
        "INSERT INTO history_objects(object_id,run_id,sequence,state) VALUES(?,?,?,'pending')",
    )
    .bind(id)
    .bind(attempt.run_id.as_str())
    .bind(i64::try_from(sequence).map_err(|_| StoreError::Conflict)?)
    .execute(connection)
    .await
    .map_err(database)?;
    Ok(())
}
pub(super) async fn bind(
    connection: &mut SqliteConnection,
    run: &RunId,
    sequence: u64,
    id: &str,
) -> Result<(), StoreError> {
    let changed = sqlx::query("UPDATE history_objects SET state='bound' WHERE object_id=? AND run_id=? AND sequence=? AND state='pending'")
        .bind(id).bind(run.as_str()).bind(i64::try_from(sequence).map_err(|_| StoreError::Conflict)?)
        .execute(connection).await.map_err(database)?;
    if changed.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(super) async fn claim_collection(
    connection: &mut SqliteConnection,
    after: String,
) -> Result<Vec<String>, StoreError> {
    // The caller holds exclusive store access: no pending writer can later bind.
    let ids: Vec<String> = sqlx::query_scalar("SELECT object_id FROM history_objects WHERE state IN ('pending','deleting') AND object_id>? ORDER BY object_id LIMIT 100")
        .bind(after).fetch_all(&mut *connection).await.map_err(database)?;
    for id in &ids {
        sqlx::query(
            "UPDATE history_objects SET state='deleting' WHERE object_id=? AND state='pending'",
        )
        .bind(id)
        .execute(&mut *connection)
        .await
        .map_err(database)?;
    }
    Ok(ids)
}
pub(super) async fn finish_collection(
    connection: &mut SqliteConnection,
    id: String,
) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM history_objects WHERE object_id=? AND state='deleting'")
        .bind(id)
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(())
}
