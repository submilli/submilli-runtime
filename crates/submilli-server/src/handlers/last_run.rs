use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::app::AppState;
use crate::session::StoredLastRun;

/// The session's last run, as the JSON it was recorded as.
pub async fn handle(State(state): State<AppState>, Path(session_id): Path<String>) -> Response {
    match state.last_runs().get(&session_id).await {
        Ok(Some(StoredLastRun::Json(json))) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response(),
        // Only its size was kept: answering 404 would read as no run, and
        // anything less than the whole run as all of it.
        Ok(Some(StoredLastRun::TooLarge { bytes, limit })) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            axum::Json(json!({"error": "last_run_too_large", "bytes": bytes, "limit": limit})),
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::warn!(operation = "get", session = %session_id, %error, "last-run storage failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
