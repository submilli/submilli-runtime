//! Admin endpoints backing `submilli server status` / `submilli server stop`.
//!
//! `GET /v1/status` reports liveness, the bound address, pid, active session
//! count, and loaded blueprint names. `POST /v1/shutdown` signals the server to
//! drain and exit. Both are unauthenticated; acceptable on the default loopback
//! bind, but a production deployment should put them behind a dedicated admin
//! surface (future hardening).

use axum::extract::State;
use axum::{Json, http::StatusCode};
use serde::Serialize;

use crate::app::AppState;

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub bind_addr: Option<String>,
    pub pid: u32,
    pub active_sessions: usize,
    pub blueprints: Vec<String>,
}

pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "running",
        bind_addr: state.bind_addr().map(|a| a.to_string()),
        pid: std::process::id(),
        active_sessions: state.session_manager().active_count(),
        blueprints: state.blueprints().list().await,
    })
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
