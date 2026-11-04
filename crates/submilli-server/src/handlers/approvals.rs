//! Calls waiting for an approver, as an administrator decides them:
//!
//! * `GET  /v1/approvals`           — every pending approval, oldest first
//! * `GET  /v1/approvals/{call_id}` — one
//! * `POST /v1/approvals/{call_id}` — decide it, as `POST /v1/runs/{id}/resume` would
//!
//! The run registry holds them: a durable run that has not replayed to its call
//! since a restart is listed once it has.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use interpreter::runtime::Approver;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::handlers::runs::{decided, deliver};
use crate::runs::{Decision, PendingApproval};

/// `GET /v1/approvals` filters.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub approver: Option<Approver>,
    pub session: Option<String>,
}

/// An administrator's decision on one call.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideRequest {
    pub decision: Decision,
    #[serde(default)]
    pub reason: Option<String>,
}

/// A pending approval, as an administrator is shown it.
#[derive(Serialize)]
struct Shown<'a> {
    call_id: &'a str,
    execution_id: &'a str,
    session_id: &'a str,
    approval: &'a crate::runs::ApprovalView,
    /// The session's variables, when it is still open.
    variables: Option<std::collections::BTreeMap<String, String>>,
}

/// `GET /v1/approvals`'s answer.
#[derive(Serialize)]
struct Listed<'a> {
    approvals: Vec<Shown<'a>>,
}

pub async fn list(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> axum::response::Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                None,
                error.body_text(),
            );
        }
    };
    let pending: Vec<_> = state
        .session_manager()
        .runs()
        .pending_approvals()
        .into_iter()
        .filter(|pending| {
            query
                .approver
                .is_none_or(|approver| pending.approval.request.approver == approver)
                && query
                    .session
                    .as_deref()
                    .is_none_or(|session| pending.entry.session_id == session)
        })
        .collect();
    let mut approvals = Vec::with_capacity(pending.len());
    for pending in &pending {
        approvals.push(shown(&state, pending).await);
    }
    Json(Listed { approvals }).into_response()
}

pub async fn show(
    State(state): State<AppState>,
    Path(call_id): Path<String>,
) -> axum::response::Response {
    match state.session_manager().runs().pending_approval(&call_id) {
        Some(pending) => Json(shown(&state, &pending).await).into_response(),
        None => unknown_approval(&call_id),
    }
}

pub async fn decide(
    State(state): State<AppState>,
    Path(call_id): Path<String>,
    request: Result<Json<DecideRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return refusal(
                error.status(),
                "invalid_request",
                Some(&call_id),
                error.body_text(),
            );
        }
    };
    let Some(pending) = state.session_manager().runs().pending_approval(&call_id) else {
        return unknown_approval(&call_id);
    };
    let answer = match decided(call_id.clone(), request.decision, request.reason) {
        Ok(answer) => answer,
        Err(message) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                Some(&call_id),
                message,
            );
        }
    };
    // The route is for administrators, so the caller may decide either approver.
    deliver(&state, &pending.entry.id, vec![answer], true).await
}

async fn shown<'a>(state: &AppState, pending: &'a PendingApproval) -> Shown<'a> {
    let variables = state
        .session_manager()
        .execution_bindings(&pending.entry.session_id)
        .await
        .ok()
        .map(|(binding, _)| binding.variables().clone());
    Shown {
        call_id: &pending.call_id,
        execution_id: &pending.entry.id,
        session_id: &pending.entry.session_id,
        approval: &pending.approval,
        variables,
    }
}

fn unknown_approval(call_id: &str) -> axum::response::Response {
    refusal(
        StatusCode::NOT_FOUND,
        "unknown_approval",
        Some(call_id),
        "no call waits for approval under this id: it was decided, it expired, its run \
         ended, or it never existed"
            .to_owned(),
    )
}

fn refusal(
    status: StatusCode,
    error: &str,
    call_id: Option<&str>,
    message: String,
) -> axum::response::Response {
    let mut body = serde_json::json!({ "error": error, "message": message });
    if let Some(call_id) = call_id {
        body["call_id"] = serde_json::json!(call_id);
    }
    (status, Json(body)).into_response()
}
