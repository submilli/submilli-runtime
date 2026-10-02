//! `GET /v1/volumes` — the operator-declared volume names a blueprint's `vfs`
//! can name, as a `named` root or under `mounts`. Read-only, and names only.
//!
//! Enumerating the declared set to any caller is deliberate: volume names are
//! not a security boundary — any caller able to register a blueprint can name
//! any declared volume and gets the access its declaration allows. What must never
//! leave the server is the host directory behind a name, so this response
//! carries names alone.

use axum::{Json, extract::State};
use serde::Serialize;

use crate::app::AppState;

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub volumes: Vec<String>,
}

/// `GET /v1/volumes` — list declared volume names (never their host
/// directories). A server with none declared answers with an empty list.
pub async fn list(State(state): State<AppState>) -> Json<ListResponse> {
    let volumes = state.volumes().keys().cloned().collect();
    Json(ListResponse { volumes })
}
