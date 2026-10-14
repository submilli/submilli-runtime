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

fn store_failure(e: SecretStoreError) -> Failure {
    match e {
        SecretStoreError::InvalidName(message) => invalid_name(message),
        e => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            e.to_string(),
        ),
    }
}

fn invalid_name(message: String) -> Failure {
    err(StatusCode::BAD_REQUEST, "invalid_secret_name", message)
}

/// Refuse a name no secret can have, whichever store is installed: the CLI's
/// store keeps MCP grants under the reserved prefix.
fn check_name(key: &str) -> Result<(), Failure> {
    crate::domain::secret::SecretName::parse(key.to_owned())
        .map(|_| ())
        .map_err(|error| invalid_name(error.to_string()))
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
    check_name(&req.key)?;
    store(&state)?
        .put(&req.key, &req.value)
        .await
        .map_err(store_failure)?;
    Ok((StatusCode::OK, Json(PutResponse { key: req.key })))
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub key: String,
}

/// `DELETE /v1/secrets/{*key}` — remove a secret, or return 404 if absent.
pub async fn remove(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<DeleteResponse>, Failure> {
    crate::audit::annotate(serde_json::json!({"key": key}));
    check_name(&key)?;
    if !store(&state)?.delete(&key).await.map_err(store_failure)? {
        return Err(err(
            StatusCode::NOT_FOUND,
            "secret_not_found",
            format!("secret '{key}' was not found"),
        ));
    }
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

/// `GET /v1/secrets?prefix=` — list keys (never values). MCP grants, which the
/// CLI's store keeps among its secrets, are listed as grants instead.
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, Failure> {
    let mut keys = store(&state)?
        .list(query.prefix.as_deref())
        .await
        .map_err(store_failure)?;
    keys.retain(|key| !key.starts_with(submilli_shared::mcp_auth::GRANT_KEY_PREFIX));
    Ok(Json(ListResponse { keys }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn secret_names_reserve_the_grant_store_prefix() {
        assert_eq!(
            crate::domain::secret::RESERVED_PREFIX,
            submilli_shared::mcp_auth::GRANT_KEY_PREFIX
        );
    }
}
