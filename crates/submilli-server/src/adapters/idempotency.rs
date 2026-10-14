//! Relational mapping for idempotent requests. Transactions belong to the caller.
use crate::database::DatabaseError;
use crate::domain::idempotent_request::{IdempotentRequest, RecordedOutcome, RequestState};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, UNIX_EPOCH};

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
    let created = request
        .created_at()
        .duration_since(UNIX_EPOCH)
        .map_err(invalid)?;
    let created = i64::try_from(created.as_millis()).map_err(invalid)?;
    let (state, status, body) = match request.state() {
        RequestState::Reserved => ("reserved", None, None),
        RequestState::Indeterminate => ("indeterminate", None, None),
        RequestState::Completed(outcome) => (
            "completed",
            Some(outcome.status),
            Some(outcome.body.as_bytes()),
        ),
    };
    sqlx::query("INSERT INTO idempotent_requests(session_id,request_key,fingerprint,reservation_id,owner_generation,created_at_unix_ms,state,response_status,response_body) VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(session_id,request_key) DO UPDATE SET state=excluded.state,response_status=excluded.response_status,response_body=excluded.response_body WHERE idempotent_requests.reservation_id=excluded.reservation_id")
        .bind(request.session_id()).bind(request.key()).bind(request.fingerprint()).bind(request.reservation_id()).bind(request.owner_generation()).bind(created).bind(state).bind(status).bind(body).execute(connection).await?;
    Ok(())
}

fn decode(row: SqliteRow) -> Result<IdempotentRequest, DatabaseError> {
    let millis: i64 = row.try_get("created_at_unix_ms")?;
    let time = UNIX_EPOCH
        .checked_add(Duration::from_millis(
            u64::try_from(millis).map_err(invalid)?,
        ))
        .ok_or_else(|| invalid("request timestamp overflow"))?;
    let mut request = IdempotentRequest::reserve(
        row.try_get("session_id")?,
        row.try_get("request_key")?,
        row.try_get("fingerprint")?,
        row.try_get("reservation_id")?,
        row.try_get("owner_generation")?,
        time,
    )
    .map_err(invalid)?;
    let identity = request.reservation_id().to_owned();
    match row.try_get::<&str, _>("state")? {
        "reserved" => {}
        "indeterminate" => request.mark_indeterminate(&identity).map_err(invalid)?,
        "completed" => {
            let body: Vec<u8> = row.try_get("response_body")?;
            request
                .complete(
                    &identity,
                    RecordedOutcome {
                        status: row.try_get("response_status")?,
                        body: String::from_utf8(body).map_err(invalid)?,
                    },
                )
                .map_err(invalid)?;
        }
        _ => return Err(invalid("invalid request state")),
    }
    Ok(request)
}
fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(format!("invalid idempotent request: {error}"))
}
