use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;

use crate::app::AppState;
use crate::error::ExecuteError;

#[derive(Debug, Serialize)]
pub struct LastRunResponse {
    pub result: Option<String>,
    pub console: Vec<String>,
    pub error: Option<ExecuteError>,
}

pub async fn handle(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<LastRunResponse>, StatusCode> {
    match state.sessions().get(&session_id).await {
        Some(run) => Ok(Json(LastRunResponse {
            result: run.result,
            console: run.console,
            error: run.error,
        })),
        None => Err(StatusCode::NOT_FOUND),
    }
}
