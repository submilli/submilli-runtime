//! Admin REST surface for the secret store, backing the `submilli server secret`
//! CLI. Write-only: secrets can be stored, listed (names only), and deleted, but
//! never read back over the API — values are only ever read in-process by the
//! runtime. Only the secret *value* travels in a request body, never the
//! encryption key, which the server reads from its configured key source.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use submilli_shared::secret_store::{SecretStore, SecretStoreError};

use crate::app::AppState;

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
    pub message: String,
}

type Failure = (StatusCode, Json<ErrorResponse>);

fn err(status: StatusCode, error: &'static str, message: String) -> Failure {
    (status, Json(ErrorResponse { error, message }))
}

/// The configured store, or a 503 if none is installed.
fn store(state: &AppState) -> Result<&std::sync::Arc<dyn SecretStore>, Failure> {
    state.secret_store().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_secret_store",
            "no secret store is configured on this server".into(),
        )
    })
}

fn internal(e: SecretStoreError) -> Failure {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        e.to_string(),
    )
}

#[derive(Debug, Deserialize)]
pub struct PutRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct PutResponse {
    pub key: String,
}

/// `POST /v1/secrets` — store a secret (create or overwrite).
pub async fn put(
    State(state): State<AppState>,
    Json(req): Json<PutRequest>,
) -> Result<(StatusCode, Json<PutResponse>), Failure> {
    crate::audit::annotate(serde_json::json!({"key": req.key}));
    store(&state)?
        .put(&req.key, &req.value)
        .await
        .map_err(internal)?;
    Ok((StatusCode::OK, Json(PutResponse { key: req.key })))
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub key: String,
}

/// `DELETE /v1/secrets/{*key}` — remove a secret (idempotent).
pub async fn remove(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<DeleteResponse>, Failure> {
    crate::audit::annotate(serde_json::json!({"key": key}));
    store(&state)?.delete(&key).await.map_err(internal)?;
    Ok(Json(DeleteResponse { key }))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub prefix: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub keys: Vec<String>,
}

/// `GET /v1/secrets?prefix=` — list keys (never values).
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, Failure> {
    let keys = store(&state)?
        .list(query.prefix.as_deref())
        .await
        .map_err(internal)?;
    Ok(Json(ListResponse { keys }))
}
