//! Admin endpoints backing `submilli server status` / `submilli server stop`.
//!
//! `GET /v1/status` reports the bound address, pid, active session count, and
//! loaded blueprint names. `POST /v1/shutdown` signals the server to drain and
//! exit. `GET /healthz` is the liveness probe: it needs no token, so it says
//! nothing beyond "the server is answering".

use axum::extract::State;
use axum::{Json, http::StatusCode};
use serde::Serialize;

use crate::app::AppState;

pub async fn healthz() -> StatusCode {
    StatusCode::OK
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub bind_addr: Option<String>,
    pub pid: u32,
    pub active_sessions: usize,
    pub blueprints: Vec<String>,
}

pub async fn status(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    Ok(Json(StatusResponse {
        status: "running",
        bind_addr: state.bind_addr().map(|a| a.to_string()),
        pid: std::process::id(),
        active_sessions: state.session_manager().active_count().await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "session storage unavailable"})),
            )
        })?,
        blueprints: state
            .blueprints()
            .list()
            .await
            .map_err(crate::blueprint::store_failure_response)?,
    }))
}

#[derive(Debug, Serialize)]
pub struct ShutdownResponse {
    pub status: &'static str,
}

pub async fn shutdown(State(state): State<AppState>) -> (StatusCode, Json<ShutdownResponse>) {
    state.shutdown_signal().notify_one();
    (
        StatusCode::OK,
        Json(ShutdownResponse {
            status: "shutting-down",
        }),
    )
}
