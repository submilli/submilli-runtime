use super::*;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};

pub(crate) async fn load(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<SessionRecord>, DatabaseError> {
    let row = sqlx::query("SELECT *, EXISTS(SELECT 1 FROM session_cleanup c WHERE c.session_id=sessions.session_id) AS cleanup_pending FROM sessions WHERE session_id=?")
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
    match row {
        Some(row) => decode(connection, row).await.map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn load_all(
    connection: &mut SqliteConnection,
) -> Result<Vec<SessionRecord>, DatabaseError> {
    let rows = sqlx::query("SELECT *, EXISTS(SELECT 1 FROM session_cleanup c WHERE c.session_id=sessions.session_id) AS cleanup_pending FROM sessions ORDER BY session_id")
        .fetch_all(&mut *connection)
        .await?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(decode(connection, row).await?);
    }
    Ok(records)
}

pub(crate) async fn for_blueprint(
    connection: &mut SqliteConnection,
    name: &str,
) -> Result<Vec<SessionRecord>, DatabaseError> {
    let rows = sqlx::query("SELECT *, EXISTS(SELECT 1 FROM session_cleanup c WHERE c.session_id=sessions.session_id) AS cleanup_pending FROM sessions WHERE blueprint_name=? ORDER BY session_id").bind(name).fetch_all(&mut *connection).await?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(decode(connection, row).await?);
    }
    Ok(records)
}

pub(crate) async fn expiry_candidates(
    connection: &mut SqliteConnection,
    now: i64,
) -> Result<Vec<SessionRecord>, DatabaseError> {
    let rows = sqlx::query("SELECT *, EXISTS(SELECT 1 FROM session_cleanup c WHERE c.session_id=sessions.session_id) AS cleanup_pending FROM sessions WHERE status='active' AND last_activity_unix_ms < ? AND ? - last_activity_unix_ms > idle_timeout_ms ORDER BY session_id").bind(now).bind(now).fetch_all(&mut *connection).await?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(decode(connection, row).await?);
    }
    Ok(records)
}

async fn decode(
    connection: &mut SqliteConnection,
    row: SqliteRow,
) -> Result<SessionRecord, DatabaseError> {
    let id: String = row.try_get("session_id")?;
    let variables: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, value FROM session_variables WHERE session_id=? ORDER BY name",
    )
    .bind(&id)
    .fetch_all(&mut *connection)
    .await?;
    let root_type: Option<String> = row.try_get("root_vfs_type")?;
    let activity = millis(row.try_get("last_activity_unix_ms")?)?;
    Ok(SessionRecord {
        mcp_state: super::mcp::load(connection, &id).await?,
        session_id: id,
        revision: row.try_get("record_version")?,
        status: SessionStatus::from_storage(&row.try_get::<String, _>("status")?)
            .map_err(|error| DatabaseError::Import(error.to_string()))?,
        closed_reason: row
            .try_get::<Option<String>, _>("closed_reason")?
            .map(|value| {
                ClosedReason::from_storage(&value)
                    .map_err(|error| DatabaseError::Import(error.to_string()))
            })
            .transpose()?,
        blueprint_name: row.try_get("blueprint_name")?,
        idle_timeout: Duration::from_millis(millis(row.try_get("idle_timeout_ms")?)?),
        last_activity: UNIX_EPOCH
            .checked_add(Duration::from_millis(activity))
            .ok_or_else(|| DatabaseError::Import("session activity timestamp overflow".into()))?,
        root_vfs_type: root_type
            .as_deref()
            .map(RootVfsType::from_storage)
            .transpose()
            .map_err(|e| DatabaseError::Import(e.to_string()))?
            .unwrap_or_default(),
        legacy_root_unresolved: root_type.is_none(),
        root_vfs_path: row
            .try_get::<Option<String>, _>("root_vfs_path")?
            .map(PathBuf::from),
        cleanup_pending: row.try_get("cleanup_pending")?,
        sealed_bindings: row.try_get("encrypted_harness_bindings")?,
        ephemeral_bindings: None,
        variables: variables.into_iter().collect(),
    })
}

pub(crate) fn timestamp(value: i64) -> Result<SystemTime, DatabaseError> {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(millis(value)?))
        .ok_or_else(|| DatabaseError::Import("session timestamp overflow".into()))
}

pub(super) fn millis(value: i64) -> Result<u64, DatabaseError> {
    u64::try_from(value)
        .map_err(|_| DatabaseError::Import("negative session timestamp or timeout".into()))
}

pub(super) async fn write(
    connection: &mut SqliteConnection,
    record: &SessionRecord,
) -> Result<(), DatabaseError> {
    let activity = record
        .last_activity
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DatabaseError::Import("session activity predates Unix epoch".into()))?;
    let activity = checked_millis(activity)?;
    let timeout = checked_millis(record.idle_timeout)?;
    let workspace = record
        .root_vfs_path
        .as_ref()
        .map(|path| {
            path.to_str()
                .ok_or_else(|| DatabaseError::Import("session workspace path is not UTF-8".into()))
        })
        .transpose()?;
    sqlx::query("INSERT INTO sessions(session_id,record_version,status,closed_reason,blueprint_name,idle_timeout_ms,last_activity_unix_ms,root_vfs_type,root_vfs_path,encrypted_harness_bindings) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(session_id) DO UPDATE SET record_version=excluded.record_version,status=excluded.status,closed_reason=excluded.closed_reason,blueprint_name=excluded.blueprint_name,idle_timeout_ms=excluded.idle_timeout_ms,last_activity_unix_ms=excluded.last_activity_unix_ms,root_vfs_type=excluded.root_vfs_type,root_vfs_path=excluded.root_vfs_path,encrypted_harness_bindings=excluded.encrypted_harness_bindings")
        .bind(&record.session_id).bind(record.revision).bind(record.status.as_str()).bind(record.closed_reason.and_then(ClosedReason::as_storage))
        .bind(&record.blueprint_name).bind(timeout).bind(activity).bind((!record.legacy_root_unresolved).then(|| record.root_vfs_type.as_str()))
        .bind(workspace).bind(&record.sealed_bindings)
        .execute(&mut *connection).await?;
    if record.cleanup_pending {
        super::queue_cleanup(connection, &SessionCleanup::from_record(record)).await?;
    }
    sqlx::query("DELETE FROM session_variables WHERE session_id=?")
        .bind(&record.session_id)
        .execute(&mut *connection)
        .await?;
    for (name, value) in &record.variables {
        sqlx::query("INSERT INTO session_variables(session_id,name,value) VALUES (?,?,?)")
            .bind(&record.session_id)
            .bind(name)
            .bind(value)
            .execute(&mut *connection)
            .await?;
    }
    super::mcp::write(connection, &record.session_id, record.mcp_state.as_ref()).await
}

fn checked_millis(duration: Duration) -> Result<i64, DatabaseError> {
    i64::try_from(duration.as_millis())
        .map_err(|_| DatabaseError::Import("session duration exceeds SQLite integer range".into()))
}
