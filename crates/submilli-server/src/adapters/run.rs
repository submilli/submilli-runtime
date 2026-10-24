//! Relational mapping for program runs. Transactions belong to the caller.
use crate::database::DatabaseError;
use crate::domain::idempotent_request::{CompletedResponse, RunStatus};
use crate::domain::run::Run;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) const SELECT: &str = "SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,completed_at_unix_ms FROM runs";

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    session: &str,
    key: &str,
) -> Result<Option<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,completed_at_unix_ms FROM runs WHERE session_id=? AND request_key=?")
        .bind(session)
        .bind(key)
        .fetch_optional(connection)
        .await?
        .map(decode)
        .transpose()
}

pub(crate) async fn unfinished(
    connection: &mut SqliteConnection,
) -> Result<Vec<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,completed_at_unix_ms FROM runs WHERE recovery_mode='unrecorded' AND state IN ('reserved','running')")
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(decode)
        .collect()
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    mut request: Run,
) -> Result<(), DatabaseError> {
    if request.identity().admission_order == 0 {
        let order: i64 = sqlx::query_scalar("UPDATE sessions SET last_run_order=last_run_order+1 WHERE session_id=? AND last_run_order<9223372036854775807 RETURNING last_run_order")
            .bind(request.session_id()).fetch_one(&mut *connection).await?;
        request.assign_admission(order).map_err(invalid)?;
    }
    let created = to_unix_ms(request.created_at())?;
    let completed = request.completed_at().map(to_unix_ms).transpose()?;
    let state = stored_state(request.status());
    let result = if request.revision() == 0 {
        let recovery_mode = if request.is_durable() {
            "recorded"
        } else {
            "unrecorded"
        };
        sqlx::query("INSERT INTO runs(run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,created_at_unix_ms,state,reason,completed_at_unix_ms,recovery_mode) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)" )
        .bind(request.reservation_id().as_str()).bind(request.reservation_id().as_str())
        .bind(request.identity().execution_id.as_deref()).bind(request.session_id()).bind(request.optional_key())
        .bind(request.fingerprint()).bind(&request.identity().principal).bind(&request.identity().entry)
        .bind(request.owner_generation()).bind(request.attempt())
        .bind(request.revision()).bind(request.identity().admission_order).bind(created).bind(state)
        .bind(request.status().reason()).bind(completed).bind(recovery_mode).execute(connection).await?
    } else {
        // Updating lifecycle never serializes or rewrites the retained initial inputs.
        sqlx::query("UPDATE runs SET owner_generation=?,request_key=?,state=?,reason=?,attempt=?,revision=?,completed_at_unix_ms=? WHERE run_id=? AND revision=? AND (owner_generation=? OR (recovery_mode='recorded' AND state IN ('reserved','running','blocked') AND ?='running' AND attempt=?-1))")
            .bind(request.owner_generation()).bind(request.optional_key()).bind(state).bind(request.status().reason())
            .bind(request.attempt()).bind(request.revision()).bind(completed)
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
    Ok(sqlx::query_scalar("SELECT session_id FROM runs WHERE state='completed' AND EXISTS(SELECT 1 FROM run_results WHERE run_results.run_id=runs.run_id) GROUP BY session_id HAVING COUNT(*)>? AND MIN(completed_at_unix_ms)<=? ORDER BY session_id")
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
        "SELECT run_id,completed_at_unix_ms FROM runs WHERE session_id=? AND state='completed' AND EXISTS(SELECT 1 FROM run_results WHERE run_results.run_id=runs.run_id)",
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
        expired += sqlx::query("UPDATE runs SET state='expired',revision=revision+1 WHERE session_id=? AND run_id=? AND state='completed'")
            .bind(session)
            .bind(run_id)
            .execute(&mut *connection)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM run_results WHERE run_id=? AND EXISTS(SELECT 1 FROM runs WHERE run_id=? AND session_id=? AND state='expired')").bind(run_id).bind(run_id).bind(session).execute(&mut *connection).await?;
    }
    Ok(expired)
}

pub(super) fn decode(row: SqliteRow) -> Result<Run, DatabaseError> {
    let durable = match row.try_get::<&str, _>("recovery_mode")? {
        "recorded" => true,
        "unrecorded" => false,
        _ => return Err(invalid("invalid recovery mode")),
    };
    let state = decode_status(row.try_get("state")?, row.try_get("reason")?)?;
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
        durable,
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

pub(super) fn stored_state(state: RunStatus) -> &'static str {
    match state {
        RunStatus::Queued => "reserved",
        RunStatus::Stuck(_) => "blocked",
        other => other.as_str(),
    }
}

pub(crate) async fn get_run(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<Run>, DatabaseError> {
    sqlx::query("SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,completed_at_unix_ms FROM runs WHERE run_id=?")
        .bind(id)
        .fetch_optional(connection)
        .await?
        .map(decode)
        .transpose()
}

fn decode_status(state: &str, reason: Option<&str>) -> Result<RunStatus, DatabaseError> {
    use crate::domain::run_status::{IndeterminateReason, StuckReason};
    Ok(match state {
        "reserved" => RunStatus::Queued,
        "running" => RunStatus::Running,
        "blocked" => RunStatus::Stuck(match reason {
            Some("credentials_unavailable") => StuckReason::Credentials,
            Some("storage_unavailable") => StuckReason::Storage,
            Some("incompatible_environment") => StuckReason::Compatibility,
            Some("invalid_history") => StuckReason::History,
            Some("fuel_exhausted") => StuckReason::Fuel,
            Some("execution_timeout") => StuckReason::Timeout,
            Some("execution_interrupted") => StuckReason::Interrupted,
            Some("durability_disabled") => StuckReason::Disabled,
            Some("replay_unavailable") => StuckReason::ReplayUnavailable,
            _ => return Err(invalid("invalid stuck reason")),
        }),
        "cancelled" => RunStatus::Cancelled,
        "aborted" => RunStatus::Aborted,
        "completed" => RunStatus::Completed,
        "indeterminate" => RunStatus::Indeterminate(match reason {
            Some("legacy_without_history") => IndeterminateReason::Legacy,
            // Older unrecorded rows may have no separately retained reason.
            Some("interrupted_without_history") | None => IndeterminateReason::Interrupted,
            _ => return Err(invalid("invalid indeterminate reason")),
        }),
        "expired" => RunStatus::Expired,
        _ => return Err(invalid("invalid run state")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::run_status::{IndeterminateReason, StuckReason};

    #[test]
    fn status_and_reason_roundtrip_together() {
        let statuses = [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Cancelled,
            RunStatus::Aborted,
            RunStatus::Completed,
            RunStatus::Expired,
            RunStatus::Indeterminate(IndeterminateReason::Interrupted),
            RunStatus::Indeterminate(IndeterminateReason::Legacy),
            RunStatus::Stuck(StuckReason::Credentials),
            RunStatus::Stuck(StuckReason::Storage),
            RunStatus::Stuck(StuckReason::Compatibility),
            RunStatus::Stuck(StuckReason::History),
            RunStatus::Stuck(StuckReason::Fuel),
            RunStatus::Stuck(StuckReason::Timeout),
            RunStatus::Stuck(StuckReason::Interrupted),
            RunStatus::Stuck(StuckReason::Disabled),
            RunStatus::Stuck(StuckReason::ReplayUnavailable),
        ];
        for status in statuses {
            assert_eq!(
                decode_status(stored_state(status), status.reason()).unwrap(),
                status
            );
        }
        assert!(decode_status("blocked", None).is_err());
        assert!(decode_status("blocked", Some("session_closed")).is_err());
        assert!(decode_status("indeterminate", Some("fuel_exhausted")).is_err());
        assert_eq!(
            decode_status("indeterminate", None).unwrap(),
            RunStatus::Indeterminate(IndeterminateReason::Interrupted)
        );
    }
}
