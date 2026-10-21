//! Relational mapping for program runs. Transactions belong to the caller.
use crate::adapters::sealing::Sealer;
use crate::database::DatabaseError;
use crate::domain::idempotent_request::{CompletedResponse, RecordedOutcome, RequestState};
use crate::domain::run::Run;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    session: &str,
    key: &str,
) -> Result<Option<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms,CASE WHEN length(execution_manifest)<=16777344 THEN execution_manifest ELSE NULL END AS execution_manifest FROM runs WHERE session_id=? AND request_key=?")
        .bind(session)
        .bind(key)
        .fetch_optional(connection)
        .await?
        .map(|row| decode(row, sealer))
        .transpose()
}

pub(crate) async fn unfinished(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
) -> Result<Vec<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms,CASE WHEN length(execution_manifest)<=16777344 THEN execution_manifest ELSE NULL END AS execution_manifest FROM runs WHERE recovery_mode='unrecorded' AND state IN ('reserved','running')")
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(|row| decode(row, sealer))
        .collect()
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    mut request: Run,
) -> Result<(), DatabaseError> {
    if request.identity().admission_order == 0 {
        let order: i64 = sqlx::query_scalar("UPDATE sessions SET last_run_order=last_run_order+1 WHERE session_id=? AND last_run_order<9223372036854775807 RETURNING last_run_order")
            .bind(request.session_id()).fetch_one(&mut *connection).await?;
        request.assign_admission(order).map_err(invalid)?;
    }
    let created = to_unix_ms(request.created_at())?;
    let completed = request.completed_at().map(to_unix_ms).transpose()?;
    let (state, status, body) = match request.state() {
        RequestState::Reserved => ("reserved", None, None),
        RequestState::Running => ("running", None, None),
        RequestState::Blocked => ("blocked", None, None),
        RequestState::Cancelled => ("cancelled", None, None),
        RequestState::Aborted => ("aborted", None, None),
        RequestState::Finished => ("finished", None, None),
        RequestState::Indeterminate => ("indeterminate", None, None),
        RequestState::Completed(outcome) => (
            "completed",
            Some(outcome.status),
            Some(outcome.body.as_bytes()),
        ),
        RequestState::Expired => ("expired", None, None),
    };
    let result = if request.revision() == 0 {
        let manifest = request
            .program()
            .map(|program| super::run_program::seal(sealer, request.reservation_id(), program))
            .transpose()?;
        let recovery_mode = if manifest.is_some() {
            "recorded"
        } else {
            "unrecorded"
        };
        sqlx::query("INSERT INTO runs(run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms,recovery_mode,execution_manifest) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)" )
        .bind(request.reservation_id().as_str()).bind(request.reservation_id().as_str())
        .bind(request.identity().execution_id.as_deref()).bind(request.session_id()).bind(request.optional_key())
        .bind(request.fingerprint()).bind(&request.identity().principal).bind(&request.identity().entry)
        .bind(request.owner_generation()).bind(request.attempt())
        .bind(request.revision()).bind(request.identity().admission_order).bind(created).bind(state)
        .bind(reason(request.state())).bind(status).bind(body).bind(completed).bind(recovery_mode).bind(manifest).execute(connection).await?
    } else {
        // Updating lifecycle never serializes or rewrites the retained initial inputs.
        sqlx::query("UPDATE runs SET owner_generation=?,request_key=?,state=?,reason=?,attempt=?,revision=?,response_status=?,response_body=?,completed_at_unix_ms=? WHERE run_id=? AND revision=? AND (owner_generation=? OR (recovery_mode='recorded' AND state IN ('reserved','running','blocked') AND ?='running' AND attempt=?-1))")
            .bind(request.owner_generation()).bind(request.optional_key()).bind(state).bind(reason(request.state()))
            .bind(request.attempt()).bind(request.revision()).bind(status).bind(body).bind(completed)
            .bind(request.reservation_id().as_str()).bind(request.revision()-1).bind(request.owner_generation()).bind(state).bind(request.attempt())
            .execute(connection).await?
    };
    if result.rows_affected() != 1 {
        return Err(DatabaseError::SessionConflict);
    }

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
    Ok(sqlx::query_scalar("SELECT session_id FROM runs WHERE state='completed' GROUP BY session_id HAVING COUNT(*)>? AND MIN(completed_at_unix_ms)<=? ORDER BY session_id")
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
    sqlx::query(
        "SELECT run_id,completed_at_unix_ms FROM runs WHERE session_id=? AND state='completed'",
    )
    .bind(session)
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CompletedResponse {
            run_id: row.try_get("run_id")?,
            completed_at: from_unix_ms(row.try_get("completed_at_unix_ms")?)?,
        })
    })
    .collect()
}

/// Drop the bodies of `run_ids`, keeping each as an expired tombstone. A request
/// that is no longer completed is left as it is.
pub(crate) async fn expire_responses(
    connection: &mut SqliteConnection,
    session: &str,
    run_ids: &[String],
) -> Result<u64, DatabaseError> {
    let mut expired = 0;
    for run_id in run_ids {
        expired += sqlx::query("UPDATE runs SET state='expired',revision=revision+1,response_status=NULL,response_body=NULL WHERE session_id=? AND run_id=? AND state='completed'")
            .bind(session)
            .bind(run_id)
            .execute(&mut *connection)
            .await?
            .rows_affected();
    }
    Ok(expired)
}

fn decode(row: SqliteRow, sealer: &Sealer) -> Result<Run, DatabaseError> {
    let id: String = row.try_get("run_id")?;
    let program = match row.try_get::<&str, _>("recovery_mode")? {
        "unrecorded" => None,
        "recorded" => Some(std::sync::Arc::new(super::run_program::open(
            sealer,
            &id,
            row.try_get::<&[u8], _>("execution_manifest")?,
        )?)),
        _ => return Err(invalid("invalid recovery mode")),
    };
    let state = match row.try_get::<&str, _>("state")? {
        "reserved" => RequestState::Reserved,
        "running" => RequestState::Running,
        "blocked" => RequestState::Blocked,
        "cancelled" => RequestState::Cancelled,
        "aborted" => RequestState::Aborted,
        "finished" => RequestState::Finished,
        "indeterminate" => RequestState::Indeterminate,
        "completed" => {
            let body: Vec<u8> = row.try_get("response_body")?;
            RequestState::Completed(RecordedOutcome {
                status: row.try_get("response_status")?,
                body: String::from_utf8(body).map_err(invalid)?,
            })
        }
        "expired" => RequestState::Expired,
        _ => return Err(invalid("invalid request state")),
    };
    let completed_at = row
        .try_get::<Option<i64>, _>("completed_at_unix_ms")?
        .map(from_unix_ms)
        .transpose()?;
    let key: Option<String> = row.try_get("request_key")?;
    Run::restore(
        row.try_get("session_id")?,
        key,
        row.try_get("fingerprint")?,
        row.try_get::<String, _>("reservation_id")?.into(),
        row.try_get("owner_generation")?,
        from_unix_ms(row.try_get("created_at_unix_ms")?)?,
        state,
        completed_at,
        crate::domain::run::RunIdentity {
            execution_id: row
                .try_get::<Option<String>, _>("execution_id")?
                .map(Into::into),
            principal: row.try_get("principal")?,
            entry: row.try_get("entry_point")?,
            admission_order: row.try_get("admission_order")?,
        },
        row.try_get("revision")?,
        row.try_get("attempt")?,
        u64::try_from(row.try_get::<i64, _>("history_position")?).map_err(invalid)?,
        program,
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
    DatabaseError::Import(format!("invalid run: {error}"))
}

fn reason(state: &RequestState) -> Option<&'static str> {
    match state {
        RequestState::Indeterminate => Some("interrupted_without_history"),
        RequestState::Blocked => Some("replay_unavailable"),
        RequestState::Cancelled => Some("session_closed"),
        RequestState::Aborted => Some("never_dispatched"),
        _ => None,
    }
}

pub(crate) async fn get_run(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
) -> Result<Option<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms,CASE WHEN length(execution_manifest)<=16777344 THEN execution_manifest ELSE NULL END AS execution_manifest FROM runs WHERE run_id=?")
        .bind(id)
        .fetch_optional(connection)
        .await?
        .map(|row| decode(row, sealer))
        .transpose()
}
