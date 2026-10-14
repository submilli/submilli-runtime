//! Relational mapping for idempotent requests. Transactions belong to the caller.
use crate::database::DatabaseError;
use crate::domain::idempotent_request::{
    CompletedResponse, IdempotentRequest, RecordedOutcome, RequestState,
};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    session: &str,
    key: &str,
) -> Result<Option<IdempotentRequest>, DatabaseError> {
    sqlx::query("SELECT * FROM idempotent_requests WHERE session_id=? AND request_key=?")
        .bind(session)
        .bind(key)
        .fetch_optional(connection)
        .await?
        .map(decode)
        .transpose()
}

pub(crate) async fn unfinished(
    connection: &mut SqliteConnection,
) -> Result<Vec<IdempotentRequest>, DatabaseError> {
    sqlx::query("SELECT * FROM idempotent_requests WHERE state='reserved'")
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(decode)
        .collect()
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    request: IdempotentRequest,
) -> Result<(), DatabaseError> {
    let created = to_unix_ms(request.created_at())?;
    let completed = request.completed_at().map(to_unix_ms).transpose()?;
    let (state, status, body) = match request.state() {
        RequestState::Reserved => ("reserved", None, None),
        RequestState::Indeterminate => ("indeterminate", None, None),
        RequestState::Completed(outcome) => (
            "completed",
            Some(outcome.status),
            Some(outcome.body.as_bytes()),
        ),
        RequestState::Expired { status } => ("expired", Some(*status), None),
    };
    sqlx::query("INSERT INTO idempotent_requests(session_id,request_key,fingerprint,reservation_id,owner_generation,created_at_unix_ms,state,response_status,response_body,completed_at_unix_ms) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(session_id,request_key) DO UPDATE SET state=excluded.state,response_status=excluded.response_status,response_body=excluded.response_body,completed_at_unix_ms=excluded.completed_at_unix_ms WHERE idempotent_requests.reservation_id=excluded.reservation_id")
        .bind(request.session_id()).bind(request.key()).bind(request.fingerprint()).bind(request.reservation_id()).bind(request.owner_generation()).bind(created).bind(state).bind(status).bind(body).bind(completed).execute(connection).await?;
    Ok(())
}

/// The sessions holding more than `kept` completed responses, at least one of
/// them completed at or before `cutoff`: the only ones with any to expire.
pub(crate) async fn sessions_with_responses_to_expire(
    connection: &mut SqliteConnection,
    cutoff: SystemTime,
    kept: usize,
) -> Result<Vec<String>, DatabaseError> {
    let kept = i64::try_from(kept).unwrap_or(i64::MAX);
    Ok(sqlx::query_scalar("SELECT session_id FROM idempotent_requests WHERE state='completed' GROUP BY session_id HAVING COUNT(*)>? AND MIN(completed_at_unix_ms)<=? ORDER BY session_id")
        .bind(kept)
        .bind(to_unix_ms(cutoff)?)
        .fetch_all(connection)
        .await?)
}

/// A session's completed responses, without their bodies.
pub(crate) async fn completed_responses(
    connection: &mut SqliteConnection,
    session: &str,
) -> Result<Vec<CompletedResponse>, DatabaseError> {
    sqlx::query("SELECT request_key,completed_at_unix_ms FROM idempotent_requests WHERE session_id=? AND state='completed'")
        .bind(session)
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(|row| {
            Ok(CompletedResponse {
                key: row.try_get("request_key")?,
                completed_at: from_unix_ms(row.try_get("completed_at_unix_ms")?)?,
            })
        })
        .collect()
}

/// Drop the bodies of `keys`, keeping each as an expired tombstone. A request
/// that is no longer completed is left as it is.
pub(crate) async fn expire_responses(
    connection: &mut SqliteConnection,
    session: &str,
    keys: &[String],
) -> Result<u64, DatabaseError> {
    let mut expired = 0;
    for key in keys {
        expired += sqlx::query("UPDATE idempotent_requests SET state='expired',response_body=NULL WHERE session_id=? AND request_key=? AND state='completed'")
            .bind(session)
            .bind(key)
            .execute(&mut *connection)
            .await?
            .rows_affected();
    }
    Ok(expired)
}

fn decode(row: SqliteRow) -> Result<IdempotentRequest, DatabaseError> {
    let state = match row.try_get::<&str, _>("state")? {
        "reserved" => RequestState::Reserved,
        "indeterminate" => RequestState::Indeterminate,
        "completed" => {
            let body: Vec<u8> = row.try_get("response_body")?;
            RequestState::Completed(RecordedOutcome {
                status: row.try_get("response_status")?,
                body: String::from_utf8(body).map_err(invalid)?,
            })
        }
        "expired" => RequestState::Expired {
            status: row.try_get("response_status")?,
        },
        _ => return Err(invalid("invalid request state")),
    };
    let completed_at = row
        .try_get::<Option<i64>, _>("completed_at_unix_ms")?
        .map(from_unix_ms)
        .transpose()?;
    IdempotentRequest::restore(
        row.try_get("session_id")?,
        row.try_get("request_key")?,
        row.try_get("fingerprint")?,
        row.try_get("reservation_id")?,
        row.try_get("owner_generation")?,
        from_unix_ms(row.try_get("created_at_unix_ms")?)?,
        state,
        completed_at,
    )
    .map_err(invalid)
}

fn to_unix_ms(time: SystemTime) -> Result<i64, DatabaseError> {
    let elapsed = time.duration_since(UNIX_EPOCH).map_err(invalid)?;
    i64::try_from(elapsed.as_millis()).map_err(invalid)
}

fn from_unix_ms(millis: i64) -> Result<SystemTime, DatabaseError> {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(
            u64::try_from(millis).map_err(invalid)?,
        ))
        .ok_or_else(|| invalid("request timestamp overflow"))
}

fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(format!("invalid idempotent request: {error}"))
}
