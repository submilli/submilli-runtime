//! Persistent run observation and explicit recovery, independent of tool answers.
use crate::adapters::run_result::{RunResponse, RunResponses};
use crate::app::AppState;
use crate::application::error::StoreError;
use crate::application::sessions::recovery::{self, RunQuery, RunStatus, RunStatusFilter};
use crate::domain::run::Run;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use interpreter::runtime::durable::RunId;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Default, Deserialize)]
pub(crate) struct ListQuery {
    session: Option<String>,
    status: Option<String>,
    after: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Response {
    let Ok(status) = query.status.as_deref().map(parse_status).transpose() else {
        return (StatusCode::BAD_REQUEST, "unknown run status").into_response();
    };
    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let result = async {
        let mut unit = state.units().begin().await?;
        unit.list_runs(RunQuery {
            session: query.session,
            status,
            after: query.after.map(Into::into),
            limit,
            recovery: None,
            unfinished_only: false,
        })
        .await
    }
    .await;
    match result {
        Ok(runs) => {
            let next = (runs.len() == limit)
                .then(|| runs.last().map(|run| run.reservation_id().clone()))
                .flatten();
            Json(json!({"runs": runs.iter().map(view).collect::<Vec<_>>(), "next": next}))
                .into_response()
        }
        Err(error) => unavailable(error),
    }
}

pub(crate) async fn get(state: &AppState, id: &str) -> Response {
    match observe(state, id).await {
        Ok(Some(value)) => {
            // A committed cancellation or terminal result takes precedence over
            // a pending call whose in-memory cleanup has not run yet.
            if matches!(value["status"].as_str(), Some("queued" | "running"))
                && let Some(entry) = state.session_manager().runs().get(id)
                && let pending @ crate::runs::RunState::Suspended(_) = entry.current()
            {
                return super::runs::state_response(&entry, pending);
            }
            Json(value).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "unknown run").into_response(),
        Err(error) => unavailable(error),
    }
}

pub(crate) async fn observe(state: &AppState, id: &str) -> Result<Option<Value>, StoreError> {
    let Some(observed) = responses(state)?.observe(id).await? else {
        return Ok(None);
    };
    let mut result = view(&observed.run);
    if let Some(outcome) = observed.response {
        result["response"] =
            serde_json::from_str(&outcome.body).map_err(|e| StoreError::Io(e.to_string()))?;
    }
    Ok(Some(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoverRequest {
    revision: i64,
}

pub(crate) async fn recover(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RecoverRequest>,
) -> Response {
    if !state.durable_execution() {
        return (StatusCode::CONFLICT, "durable execution is disabled").into_response();
    }
    match recovery::resume(
        state.units().as_ref(),
        &RunId::from(id.clone()),
        request.revision,
    )
    .await
    {
        Ok(()) => {
            state.wake_recovery();
            get(&state, &id).await
        }
        Err(StoreError::Conflict) => (
            StatusCode::CONFLICT,
            "run changed or is not stuck; inspect it again",
        )
            .into_response(),
        Err(error) => unavailable(error),
    }
}

/// Terminal HTTP responses retain their original serialization and status.
pub(crate) enum WaitResponse {
    Completed(RunResponse),
    Metadata(Value),
}

impl WaitResponse {
    pub fn into_json(self) -> Result<Value, serde_json::Error> {
        match self {
            Self::Completed(outcome) => serde_json::from_str(&outcome.body),
            Self::Metadata(value) => Ok(value),
        }
    }
}

/// Running programs that may suspend (at a tool call, or for an approval) with a
/// local attempt wait for suspension or completion. All other runs, including
/// queued ones that may suspend, use a bounded observer.
/// Its supervisor keeps running when this future or its transport disappears.
pub(crate) async fn wait(state: &AppState, id: &RunId) -> Result<WaitResponse, String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(25);
    let mut tools = None;
    let responses = responses(state).map_err(|e| e.to_string())?;
    let draining = state.graceful_shutdown().draining();
    loop {
        if draining.is_cancelled() {
            return Err("server is shutting down".into());
        }
        let observed = responses
            .observe(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("unknown run")?;
        if let Some(response) = observed.response {
            return Ok(WaitResponse::Completed(response));
        }
        let run = observed.run;
        if !matches!(run.status(), RunStatus::Queued | RunStatus::Running) {
            return Ok(WaitResponse::Metadata(view(&run)));
        }
        let entry = state.session_manager().runs().get(id);
        if let Some(entry) = &entry
            && let crate::runs::RunState::Suspended(calls) = entry.current()
        {
            return Ok(WaitResponse::Metadata(
                json!({"status": "suspended", "execution_id": id, "session_id": entry.session_id, "calls": calls}),
            ));
        }
        let has_tools = if let Some(tools) = tools {
            tools
        } else {
            let mut unit = state.units().begin().await.map_err(|e| e.to_string())?;
            let present = unit
                .get_run_program(id)
                .await
                .map_err(|e| e.to_string())?
                .is_some_and(|program| {
                    program.tools.is_some() || (program.approvals && program.blueprint.asks())
                });
            tools = Some(present);
            present
        };
        let owned_tool_run = has_tools && run.status() == RunStatus::Running && entry.is_some();
        if !owned_tool_run && tokio::time::Instant::now() >= deadline {
            return Ok(WaitResponse::Metadata(view(&run)));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub(crate) fn responses(state: &AppState) -> Result<RunResponses, StoreError> {
    let database = state
        .database()
        .ok_or_else(|| StoreError::Io("run storage unavailable".into()))?;
    Ok(RunResponses::new(database.clone(), state.secret_cipher()))
}

pub(crate) fn view(run: &Run) -> Value {
    let millis = |time: std::time::SystemTime| {
        time.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis())
    };
    json!({
        "execution_id": run.reservation_id(), "session_id": run.session_id(),
        "status": run.status().as_str(), "reason": run.status().reason(),
        "durable": run.is_durable(), "revision": run.revision(), "attempt": run.attempt(),
        "admission_order": run.identity().admission_order,
        "created_at_unix_ms": millis(run.created_at()),
        "completed_at_unix_ms": run.completed_at().and_then(millis),
    })
}

fn parse_status(status: &str) -> Result<RunStatusFilter, ()> {
    match status {
        "queued" => Ok(RunStatusFilter::Queued),
        "running" => Ok(RunStatusFilter::Running),
        "stuck" => Ok(RunStatusFilter::Stuck),
        "completed" => Ok(RunStatusFilter::Completed),
        "cancelled" => Ok(RunStatusFilter::Cancelled),
        "aborted" => Ok(RunStatusFilter::Aborted),
        "indeterminate" => Ok(RunStatusFilter::Indeterminate),
        "expired" => Ok(RunStatusFilter::Expired),
        _ => Err(()),
    }
}

fn unavailable(error: StoreError) -> Response {
    tracing::warn!(%error, "run observation unavailable");
    (StatusCode::SERVICE_UNAVAILABLE, "run storage unavailable").into_response()
}
