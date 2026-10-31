//! Runs that suspend at an application tool call (`@tools`), or at a call a rule
//! asks an approver about (`ask-human`, `ask-admin`):
//!
//! * `POST /v1/runs/{id}/resume` — answer the run's pending call, then wait for
//!   it to suspend again or finish
//! * `GET  /v1/runs/{id}`        — where the run is now
//!
//! A run's id is its execution id. A session execute whose session has
//! application tools answers with the same states: `suspended` with the pending
//! calls, or `finished` with the execute result.

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use interpreter::runtime::{McpResponse, ToolCallError};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::auth::AdminCaller;
use crate::handlers::execute::{self, ExecuteResponse, Finished};
use crate::runs::{
    Answer, CallAnswer, Decision, PendingCallView, ResumeRefusal, RunEntry, RunState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequest {
    pub results: Vec<CallResult>,
}

/// The harness's answer to one call: `ok` for a tool call, `decision` for an
/// approval.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallResult {
    pub call_id: String,
    /// Whether the tool returned a value.
    #[serde(default)]
    pub ok: Option<bool>,
    /// The tool's return value, when `ok`.
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    /// Why there is no value, when not `ok`.
    #[serde(default)]
    pub error: Option<CallError>,
    /// The approver's decision.
    #[serde(default)]
    pub decision: Option<Decision>,
    /// Why, with a decision. A denial's reason reaches the program.
    #[serde(default)]
    pub reason: Option<String>,
}

/// The longest `reason` a decision may give, in bytes.
pub(crate) const MAX_REASON_BYTES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallError {
    pub kind: CallErrorKind,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallErrorKind {
    /// The tool ran and failed: `Error(message)` in the program.
    Failed,
    /// The harness, or a person, refused the call: `PermissionDeniedError`.
    Denied,
}

/// `GET /v1/runs/{id}`: where the run is now.
pub async fn get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> axum::response::Response {
    match state.session_manager().runs().get(&run_id) {
        Some(entry) if !entry.durable => state_response(&entry, entry.current()),
        _ => super::durable_runs::get(&state, &run_id).await,
    }
}

/// `POST /v1/runs/{id}/resume`: answer the pending call, then wait for the run to
/// suspend again or finish. Answering a call already answered changes nothing and
/// returns the run's state.
pub async fn resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    caller: Option<axum::Extension<AdminCaller>>,
    request: Result<Json<ResumeRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let admin = caller.is_some_and(|axum::Extension(AdminCaller(admin))| admin);
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return refusal(
                error.status(),
                "invalid_request",
                &run_id,
                error.body_text(),
            );
        }
    };
    let answers = match answers(request.results) {
        Ok(answers) => answers,
        Err(message) => {
            return refusal(StatusCode::BAD_REQUEST, "invalid_request", &run_id, message);
        }
    };
    deliver(&state, &run_id, answers, admin).await
}

/// Deliver `answers` to run `run_id`, then wait for it to suspend again or finish,
/// and answer with that state. `admin` is whether the caller may decide an
/// `ask-admin` call.
pub(crate) async fn deliver(
    state: &AppState,
    run_id: &str,
    answers: Vec<CallAnswer>,
    admin: bool,
) -> axum::response::Response {
    if let Ok(responses) = super::durable_runs::responses(state) {
        match responses.observe(run_id).await {
            Ok(Some(observed)) if observed.run.is_durable() => {
                return resume_durable(state, run_id, answers, admin).await;
            }
            Err(error) => {
                return refusal(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "run_unavailable",
                    run_id,
                    error.to_string(),
                );
            }
            _ => {}
        }
    }
    let Some(entry) = state.session_manager().runs().get(run_id) else {
        return unknown_run(run_id);
    };
    let settled = match entry.resume(answers, admin) {
        Ok(Some(generation)) => entry.settled_after(generation).await,
        Ok(None) => entry.settled().await,
        Err(ResumeRefusal::NotSuspended) => {
            return refusal(
                StatusCode::CONFLICT,
                "run_not_suspended",
                run_id,
                "the run is not waiting on this call: it finished, the call timed out, or the run was cancelled".to_owned(),
            );
        }
        Err(other) => return resume_refusal(run_id, other),
    };
    state_response(&entry, settled)
}

/// A finished run's body, as the caller, the ledger and `GET` all receive it.
pub(crate) struct FinishedBody {
    pub body: String,
    /// False for a stand-in sent when the result could not be serialized: it
    /// is never recorded as the run's outcome.
    pub recordable: bool,
}

/// `response` as a finished run's body, or a stand-in that is not recordable
/// when it cannot be serialized.
pub(crate) fn finished_body(response: &ExecuteResponse) -> FinishedBody {
    match serde_json::to_string(&Finished::new(response)) {
        Ok(body) => FinishedBody {
            body,
            recordable: true,
        },
        Err(error) => {
            tracing::error!(%error, "failed to serialize a finished run");
            let stand_in = execute::internal_failure(
                &response.execution_id,
                &response.session_id,
                "internal: the result could not be serialized".to_owned(),
            );
            FinishedBody {
                // Strings only, so this cannot fail.
                body: serde_json::to_string(&Finished::new(&stand_in)).unwrap_or_default(),
                recordable: false,
            }
        }
    }
}

/// `entry` in `state`, as a response.
pub(crate) fn state_response(entry: &RunEntry, state: RunState) -> axum::response::Response {
    let ids = RunIds {
        execution_id: &entry.id,
        session_id: &entry.session_id,
    };
    let body = match state {
        RunState::Finished(body) => Ok(body.to_string()),
        RunState::Suspended(calls) => {
            serde_json::to_string(&Unfinished::Suspended { ids, calls: &calls })
        }
        RunState::Running => serde_json::to_string(&Unfinished::Running { ids }),
        RunState::Unavailable => {
            return refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "run_unavailable",
                &entry.id,
                "the execution attempt stopped; retry or inspect the durable run".into(),
            );
        }
    };
    let body = match body {
        Ok(body) => body,
        Err(error) => {
            tracing::error!(%error, "failed to serialize a run's state");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut headers = execute::session_headers(&entry.session_id);
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    (StatusCode::OK, headers, body).into_response()
}

/// A run that has not finished, as the harness sees it. A finished run is the
/// execute result, see [`Finished`].
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Unfinished<'a> {
    /// Waiting on the harness for `calls`.
    Suspended {
        #[serde(flatten)]
        ids: RunIds<'a>,
        calls: &'a [PendingCallView],
    },
    /// Working: between a resume and the next state.
    Running {
        #[serde(flatten)]
        ids: RunIds<'a>,
    },
}

#[derive(Serialize)]
struct RunIds<'a> {
    execution_id: &'a str,
    session_id: &'a str,
}

fn refusal(
    status: StatusCode,
    error: &str,
    run_id: &str,
    message: String,
) -> axum::response::Response {
    (
        status,
        Json(serde_json::json!({ "error": error, "execution_id": run_id, "message": message })),
    )
        .into_response()
}

fn unknown_run(run_id: &str) -> axum::response::Response {
    refusal(
        StatusCode::NOT_FOUND,
        "unknown_run",
        run_id,
        "no such run: it never existed, it finished a while ago, or the server restarted"
            .to_owned(),
    )
}

/// The harness's results as answers: at least one, each call named once.
fn answers(results: Vec<CallResult>) -> Result<Vec<CallAnswer>, String> {
    if results.is_empty() {
        return Err("`results` names no call; answer the run's pending call".to_owned());
    }
    let mut seen = HashSet::new();
    results
        .into_iter()
        .map(|result| {
            if !seen.insert(result.call_id.clone()) {
                return Err(format!("call `{}` is answered twice", result.call_id));
            }
            answer(result)
        })
        .collect()
}

fn answer(result: CallResult) -> Result<CallAnswer, String> {
    let ok = match (result.ok, result.decision) {
        (Some(ok), None) => ok,
        (None, Some(decision)) => return decision_answer(result, decision),
        (Some(_), Some(_)) => {
            return Err(format!(
                "call `{}`: a result carries `ok` for a tool call or `decision` for an \
                 approval, not both",
                result.call_id
            ));
        }
        (None, None) => {
            return Err(format!(
                "call `{}`: a result carries `ok` for a tool call or `decision` for an approval",
                result.call_id
            ));
        }
    };
    if result.reason.is_some() {
        return Err(format!(
            "call `{}`: `reason` goes with a `decision`; a tool's failure carries `error.message`",
            result.call_id
        ));
    }
    let answer = match (ok, result.value, result.error) {
        (true, value, None) => {
            let mut value = value.unwrap_or(serde_json::Value::Null);
            Ok(McpResponse::take(&mut value).map_err(|_| {
                format!(
                    "the value for call `{}` is too large or too deeply nested",
                    result.call_id
                )
            })?)
        }
        (false, None, Some(error)) => Err(match error.kind {
            CallErrorKind::Failed => ToolCallError::Failed {
                message: error.message,
            },
            CallErrorKind::Denied => ToolCallError::Denied {
                message: error.message,
            },
        }),
        (true, _, Some(_)) => {
            return Err(format!(
                "call `{}`: a result with `ok: true` carries a `value`, not an `error`",
                result.call_id
            ));
        }
        (false, _, _) => {
            return Err(format!(
                "call `{}`: a result with `ok: false` carries an `error` and no `value`",
                result.call_id
            ));
        }
    };
    Ok(CallAnswer {
        call_id: result.call_id,
        answer: Answer::Tool(answer),
    })
}

fn decision_answer(result: CallResult, decision: Decision) -> Result<CallAnswer, String> {
    if result.value.is_some() || result.error.is_some() {
        return Err(format!(
            "call `{}`: a `decision` carries an optional `reason`, not a `value` or an `error`",
            result.call_id
        ));
    }
    decided(result.call_id, decision, result.reason)
}

/// The answer deciding the approval call `call_id`, made by this request's token.
pub(crate) fn decided(
    call_id: String,
    decision: Decision,
    reason: Option<String>,
) -> Result<CallAnswer, String> {
    if reason
        .as_ref()
        .is_some_and(|reason| reason.len() > MAX_REASON_BYTES)
    {
        return Err(format!(
            "call `{call_id}`: `reason` is longer than {MAX_REASON_BYTES} bytes"
        ));
    }
    Ok(CallAnswer {
        call_id,
        answer: Answer::Decision {
            decision,
            reason,
            decided_by: crate::audit::principal(),
        },
    })
}

/// Durable submissions never use the bounded execute observer: replay and commit
/// must resolve the submitted ID before any successful next-state response.
async fn resume_durable(
    state: &AppState,
    id: &str,
    answers: Vec<CallAnswer>,
    admin: bool,
) -> axum::response::Response {
    use crate::application::sessions::recovery::RunStatus;
    use crate::runs::durable::Submission;
    let mut submission = match Submission::new(id, answers, admin) {
        Ok(submission) => submission,
        Err(error) => return resume_refusal(id, error),
    };
    let responses = match super::durable_runs::responses(state) {
        Ok(responses) => responses,
        Err(error) => {
            return refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "run_unavailable",
                id,
                error.to_string(),
            );
        }
    };
    let draining = state.graceful_shutdown().draining();
    loop {
        if draining.is_cancelled() {
            return refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "run_unavailable",
                id,
                "server is shutting down".into(),
            );
        }
        let observed = match responses.observe(id).await {
            Ok(Some(observed)) => observed,
            Ok(None) => return unknown_run(id),
            Err(error) => {
                return refusal(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "run_unavailable",
                    id,
                    error.to_string(),
                );
            }
        };
        if let Some(response) = observed.response {
            return super::sessions::durable_wait_response(
                observed.run.session_id(),
                super::durable_runs::WaitResponse::Completed(response),
            );
        }
        if !matches!(
            observed.run.status(),
            RunStatus::Queued | RunStatus::Running
        ) {
            return Json(super::durable_runs::view(&observed.run)).into_response();
        }
        let entry = state.session_manager().runs().get(id);
        let generation = entry.as_ref().map(|entry| entry.generation());
        if let Some(entry) = &entry {
            match entry.resume_durable(&mut submission) {
                Ok(true) => {
                    if let settled @ RunState::Suspended(_) = entry.current() {
                        return state_response(entry, settled);
                    }
                }
                Ok(false) => {}
                Err(error) => return resume_refusal(id, error),
            }
        }
        tokio::select! {
            () = draining.cancelled() => {},
            () = async {
                if let (Some(entry), Some(generation)) = (entry, generation) {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), entry.settled_after(generation)).await;
                } else {
                    tokio::time::sleep(crate::recovery::SCHEDULER_TICK).await;
                }
            } => {},
        }
    }
}

fn resume_refusal(id: &str, error: ResumeRefusal) -> axum::response::Response {
    match error {
        ResumeRefusal::NotSuspended => refusal(
            StatusCode::CONFLICT,
            "run_not_suspended",
            id,
            "the call timed out or the execution attempt stopped".into(),
        ),
        ResumeRefusal::UnknownCall(call) => refusal(
            StatusCode::BAD_REQUEST,
            "unknown_call",
            id,
            format!("the run is not waiting on call `{call}`"),
        ),
        ResumeRefusal::NotAnApproval(call) => refusal(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            id,
            format!(
                "call `{call}` is an application tool call: answer it with `ok`, not a `decision`"
            ),
        ),
        ResumeRefusal::RequiresAdmin(call) => refusal(
            StatusCode::FORBIDDEN,
            "approval_requires_admin",
            id,
            format!(
                "call `{call}` waits for an administrator's approval (`ask-admin`); only a token \
                 with the `admin` role can decide it"
            ),
        ),
    }
}
