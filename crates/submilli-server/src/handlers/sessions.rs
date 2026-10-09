//! Session lifecycle endpoints. The MCP streamable-HTTP transport drives
//! sessions natively (see `crate::mcp`); these REST endpoints mirror it so a
//! non-MCP harness gets the same stateful sandbox:
//!
//! * `POST   /v1/sessions`            — create: bind blueprint + variables (returns id)
//! * `POST   /v1/sessions/{id}/execute` — run code in that session (code only)
//! * `POST   /v1/sessions/{id}/rebind` — replace encrypted harness secrets
//! * `DELETE /v1/sessions/{id}`       — terminate, wiping the VFS immediately

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use submilli_blueprint::{
    HarnessSecretBindings, required_harness_secrets, resolve_harness_secrets, resolve_variables,
};

use crate::app::{AppState, BlueprintForRun};
use crate::handlers::execute::{self, ExecuteInputs, SESSION_HEADER, blueprint_miss_message};
use crate::idempotency::{Refusal, Reservation};
use crate::idempotency_store::RecordedOutcome;
use crate::record::{SessionRunError, SessionStartError};

/// Opt-in replay guard on the session execute endpoint. Named to match the
/// harness, which already sends it.
const IDEMPOTENCY_HEADER: &str = "idempotency-key";

#[derive(Deserialize)]
pub struct CreateRequest {
    pub blueprint: String,
    /// Caller-supplied `${vars.NAME}` bindings, fixed for the whole session and
    /// validated against the blueprint's declarations before the session is
    /// created. A missing required (or undeclared) variable rejects the request.
    #[serde(default)]
    pub variables: Option<BTreeMap<String, String>>,
    /// Trusted harness credentials. Validated against `secrets:` declarations
    /// and encrypted in durable session storage.
    #[serde(default)]
    pub secrets: Option<HarnessSecretBindings>,
}

#[derive(Debug, Serialize)]
pub struct CreateResponse {
    pub session_id: String,
}

/// Request body for `POST /v1/sessions/{id}/execute`: code only — the blueprint
/// and variables come from the bound session (parity with the MCP execute tool).
#[derive(Debug, Deserialize)]
pub struct SessionExecuteRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct RebindRequest {
    /// Complete replacement for this session's harness-secret bindings.
    pub secrets: HarnessSecretBindings,
}

/// Create a session bound to a blueprint and its resolved variables.
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> impl IntoResponse {
    let started = start(
        &state,
        &req.blueprint,
        &req.variables.unwrap_or_default(),
        &req.secrets.unwrap_or_default(),
    )
    .await;
    match started {
        Ok(session_id) => {
            let body = CreateResponse {
                session_id: session_id.clone(),
            };
            (session_header(&session_id), Json(body)).into_response()
        }
        Err(error) => start_refusal_response(error),
    }
}

/// Creates a session bound to `blueprint_name`, its resolved variables, and its harness
/// secrets, and returns its id. Everything is validated before the session exists:
/// defaults fill, and a missing-required or undeclared name refuses the request.
pub(crate) async fn start(
    state: &AppState,
    blueprint_name: &str,
    supplied: &BTreeMap<String, String>,
    supplied_secrets: &HarnessSecretBindings,
) -> Result<String, SessionStartError> {
    let Some(blueprint) = state
        .blueprints()
        .get(blueprint_name)
        .await
        .map_err(SessionStartError::Store)?
    else {
        // One code for "this name is not runnable", whether it was never registered
        // or is registered in a form this binary can no longer parse: a client that
        // has to branch on the difference reads `message`, and one that only needs to
        // know the name is unusable keeps its existing predicate.
        let message = blueprint_miss_message(state, blueprint_name)
            .await
            .map_err(SessionStartError::Store)?;
        return Err(SessionStartError::UnknownBlueprint {
            name: blueprint_name.to_owned(),
            message,
        });
    };

    let variables = resolve_variables(&blueprint.variables, supplied)
        .map(Arc::new)
        .map_err(|error| SessionStartError::InvalidVariables(error.to_string()))?;
    blueprint
        .vfs
        .resolve(&variables)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(&blueprint, &variables).map(|_| ()))
        .map_err(|error| SessionStartError::InvalidFilesystem(error.to_string()))?;
    let secrets = resolve_harness_secrets(&blueprint.secrets, supplied_secrets)
        .map(Arc::new)
        .map_err(|error| SessionStartError::InvalidSecrets(error.to_string()))?;

    state
        .session_manager()
        .create(&blueprint, variables, secrets)
        .await
        .map_err(SessionStartError::Session)
}

/// The `POST /v1/sessions` response for a session that was not started.
fn start_refusal_response(error: SessionStartError) -> axum::response::Response {
    match error {
        SessionStartError::UnknownBlueprint { name, message } => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "unknown blueprint",
                "message": message,
                "name": name,
            })),
        )
            .into_response(),
        SessionStartError::InvalidVariables(_)
        | SessionStartError::InvalidFilesystem(_)
        | SessionStartError::InvalidSecrets(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
        SessionStartError::Store(error) => {
            crate::blueprint::store_failure_response(error).into_response()
        }
        SessionStartError::Session(error) => session_state_response(error),
    }
}

/// Run code in an existing session. The blueprint and variables are taken from
/// the session bound at create; the body carries only `code`. The path id is
/// authoritative — any `mcp-session-id` header is ignored. Unknown session → 404.
///
/// An optional `Idempotency-Key` makes the call replayable: the key is reserved
/// durably before the program runs, the outcome is recorded against it, and a
/// duplicate returns those bytes instead of executing again. A request without
/// the header behaves exactly as it did before the ledger existed.
pub async fn execute(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    request: Result<Json<SessionExecuteRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let req = match request {
        Ok(Json(req)) => req,
        Err(error) => return (error.status(), Json(serde_json::json!({
            "execution_id": crate::audit::execution_id(), "error": "invalid_request", "message": error.body_text()
        }))).into_response(),
    };
    let bound = match bound_for_run(
        &state,
        &session_id,
        &req.code,
        crate::audit::execution().as_deref(),
    )
    .await
    {
        Ok(bound) => bound,
        Err(error) => return run_refusal_response(&session_id, error),
    };
    // Every session-level refusal above happens before any reservation exists,
    // so a bad key against an unknown session reports the unknown session and
    // the ledger stays untouched. This is the last point before the
    // program can run.
    let key = match idempotency_key(&headers) {
        None => {
            let outcome = run(
                &state,
                &session_id,
                bound,
                &req.code,
                None,
                crate::audit::execution(),
            )
            .await;
            return execute::with_session_header(&session_id, outcome.response).into_response();
        }
        Some(Err(refusal)) => return refusal_response(&session_id, &refusal),
        Some(Ok(key)) => key,
    };

    let guard = match state
        .idempotency()
        .reserve(&session_id, &key, &req.code)
        .await
    {
        Reservation::Refused(refusal) => return refusal_response(&session_id, &refusal),
        Reservation::Replay(outcome) => {
            if let Some(audit) = crate::audit::execution() {
                audit.replay();
            }
            if let Some(recorder) = state.run_recorder() {
                recorder.retried(crate::record::RetryLink {
                    session_id: session_id.clone(),
                    idempotency_key: key.clone(),
                    original_execution_id: serde_json::from_str::<serde_json::Value>(&outcome.body)
                        .ok()
                        .and_then(|body| body.get("execution_id")?.as_str().map(str::to_owned)),
                });
            }
            // A replay is session activity: a client retrying must not have its
            // session reaped underneath it. `execute_core` normally does this,
            // and the replay path never reaches it.
            if let Err(error) = state.session_manager().touch(&session_id).await {
                return session_state_response(error);
            }
            return recorded_response(&session_id, &outcome);
        }
        Reservation::Proceed(guard) => guard,
    };

    let outcome = run(
        &state,
        &session_id,
        bound,
        &req.code,
        Some(&key),
        crate::audit::execution(),
    )
    .await;
    if !outcome.dispatched {
        // Nothing reached the runner, so nothing can have happened. Drop
        // the reservation rather than caching a transient failure against this
        // key forever.
        guard.release().await;
        return execute::with_session_header(&session_id, outcome.response).into_response();
    }

    // Serialize once and hand the same bytes to the ledger and the caller, so a
    // replay cannot drift from what the original client received.
    let Ok(body) = serde_json::to_string(&outcome.response) else {
        // Unreachable in practice. The program *did* run, so releasing would let
        // a retry re-run it; dropping the guard armed leaves the key
        // indeterminate, which is the honest answer.
        tracing::error!("failed to serialize execute response for the idempotency ledger");
        drop(guard);
        return execute::with_session_header(&session_id, outcome.response).into_response();
    };
    guard.complete(StatusCode::OK.as_u16(), body.clone()).await;
    stored_body_response(&session_id, StatusCode::OK, body)
}

/// What a session runs under: the blueprint it was bound to, as registered now, and the
/// variables and harness secrets fixed when it was created.
pub(crate) struct BoundSession {
    blueprint_name: String,
    found: BlueprintForRun,
    variables: Arc<submilli_blueprint::VarBindings>,
    harness_secrets: Arc<HarnessSecretBindings>,
}

/// Reads what `session_id` runs under, refusing a session that cannot run: unknown or
/// closed, bound to a blueprint that is no longer runnable, or missing the harness
/// secrets its blueprint requires. `audit` is annotated with the program as it will run.
pub(crate) async fn bound_for_run(
    state: &AppState,
    session_id: &str,
    code: &str,
    audit: Option<&crate::audit::ExecutionAudit>,
) -> Result<BoundSession, SessionRunError> {
    let Some(blueprint_name) = state
        .session_manager()
        .blueprint_name(session_id)
        .await
        .map_err(SessionRunError::Session)?
    else {
        return Err(SessionRunError::UnknownSession {
            session_id: session_id.to_owned(),
        });
    };
    let Some(found) = state
        .blueprint_for_run(&blueprint_name)
        .await
        .map_err(SessionRunError::Store)?
    else {
        // A session outlives a restart, so its blueprint may have become unrunnable
        // (rather than removed) while the session slept: `message` says which.
        let message = blueprint_miss_message(state, &blueprint_name)
            .await
            .map_err(SessionRunError::Store)?;
        return Err(SessionRunError::BlueprintMissing {
            name: blueprint_name,
            message,
        });
    };

    let blueprint = &found.blueprint;
    let (binding, secrets) = state
        .session_manager()
        .execution_bindings(session_id)
        .await
        .map_err(SessionRunError::Session)?;
    let variables = Arc::new(binding.variables().clone());
    if let Some(audit) = audit {
        audit.annotate(code, &blueprint_name, Some(blueprint), &variables);
    }
    let harness_secrets = secrets.or_else(|| {
        required_harness_secrets(&blueprint.secrets)
            .is_empty()
            .then(Arc::default)
    });
    let Some(harness_secrets) = harness_secrets else {
        return Err(SessionRunError::SecretsRequired {
            session_id: session_id.to_owned(),
            required: required_harness_secrets(&blueprint.secrets),
        });
    };
    Ok(BoundSession {
        blueprint_name,
        found,
        variables,
        harness_secrets,
    })
}

/// The `POST /v1/sessions/{id}/execute` response for a session that cannot run.
fn run_refusal_response(session_id: &str, error: SessionRunError) -> axum::response::Response {
    match error {
        SessionRunError::UnknownSession { .. } => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "execution_id": crate::audit::execution_id(), "error": "unknown session", "session_id": session_id })),
        )
            .into_response(),
        SessionRunError::BlueprintMissing { name, message } => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "execution_id": crate::audit::execution_id(),
                    "error": "blueprint no longer exists",
                    "message": message,
                    "name": name,
                }),
            ),
        )
            .into_response(),
        SessionRunError::SecretsRequired { required, .. } => (
            StatusCode::CONFLICT,
            Json(
                serde_json::json!({ "execution_id": crate::audit::execution_id(),
                    "error": "session_requires_secrets",
                    "session_id": session_id,
                    "required": required,
                }),
            ),
        )
            .into_response(),
        SessionRunError::Store(error) => execution_store_failure(error),
        SessionRunError::Session(error) => session_state_response(error),
    }
}

/// Runs `code` in `session_id` under what it is bound to, recorded as a session run.
pub(crate) async fn run(
    state: &AppState,
    session_id: &str,
    bound: BoundSession,
    code: &str,
    idempotency_key: Option<&str>,
    audit: Option<Arc<crate::audit::ExecutionAudit>>,
) -> execute::ExecuteOutcome {
    let BoundSession {
        blueprint_name,
        found,
        variables,
        harness_secrets,
    } = bound;
    execute::execute_core(
        state,
        ExecuteInputs {
            session_id,
            code,
            blueprint_name: &blueprint_name,
            blueprint: Arc::new(found.blueprint),
            version_tag: found.version_tag,
            variables,
            harness_secrets,
            audit,
            vfs_source: execute::VfsSource::Rest,
            run_entry: crate::record::RunEntry::Session,
            client: None,
            tool_call_id: None,
            idempotency_key,
        },
    )
    .await
}

/// `None` when the header is absent — the unkeyed path. A present but
/// unparseable value is refused rather than quietly treated as unkeyed, which
/// would run the program without the guard the caller asked for.
fn idempotency_key(headers: &HeaderMap) -> Option<Result<String, Refusal>> {
    let mut values = headers.get_all(IDEMPOTENCY_HEADER).iter();
    let first = values.next()?;
    // Two different keys on one request is a caller bug with no safe reading:
    // silently taking the first would run the program under a key the caller
    // may not have meant, which is exactly the guarantee they asked for.
    if values.next().is_some() {
        return Some(Err(Refusal::InvalidKey(
            "Idempotency-Key must appear at most once",
        )));
    }
    Some(
        first
            .to_str()
            .map(str::to_string)
            .map_err(|_| Refusal::InvalidKey("Idempotency-Key must be printable ASCII")),
    )
}

fn refusal_response(session_id: &str, refusal: &Refusal) -> axum::response::Response {
    (
        refusal.status(),
        Json(serde_json::json!({
            "execution_id": crate::audit::execution_id(),
            "error": refusal.code(),
            "session_id": session_id,
            "detail": refusal.detail(),
        })),
    )
        .into_response()
}

fn recorded_response(session_id: &str, outcome: &RecordedOutcome) -> axum::response::Response {
    let status = StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::OK);
    stored_body_response(session_id, status, outcome.body.clone())
}

/// Return already-serialized bytes verbatim, with the session header the
/// endpoint always sets. Bypasses `Json` so a replay is byte-identical to the
/// original rather than a re-serialization that happens to agree.
fn stored_body_response(
    session_id: &str,
    status: StatusCode,
    body: String,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.insert(HeaderName::from_static(SESSION_HEADER), value);
    }
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    (status, headers, body).into_response()
}

pub async fn rebind(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<RebindRequest>,
) -> impl IntoResponse {
    let blueprint_name = match state.session_manager().blueprint_name(&session_id).await {
        Ok(name) => name,
        Err(error) => return session_state_response(error),
    };
    let Some(blueprint_name) = blueprint_name else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown session", "session_id": session_id })),
        )
            .into_response();
    };
    let found = match state.blueprints().get(&blueprint_name).await {
        Ok(found) => found,
        Err(error) => return crate::blueprint::store_failure_response(error).into_response(),
    };
    let Some(blueprint) = found else {
        // A session outlives a restart, so its blueprint may have become unrunnable
        // (rather than removed) while the session slept: `message` says which.
        let message = match blueprint_miss_message(&state, &blueprint_name).await {
            Ok(message) => message,
            Err(error) => return crate::blueprint::store_failure_response(error).into_response(),
        };
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "blueprint no longer exists",
                "message": message,
                "name": blueprint_name,
            })),
        )
            .into_response();
    };
    let resolved = match resolve_harness_secrets(&blueprint.secrets, &req.secrets) {
        Ok(resolved) => Arc::new(resolved),
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid secrets: {err}") })),
            )
                .into_response();
        }
    };
    match state
        .session_manager()
        .rebind_harness_secrets(&session_id, resolved)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(crate::session_manager::SessionError::UnknownSession) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown session", "session_id": session_id })),
        )
            .into_response(),
        Err(error) => session_state_response(error),
    }
}

/// Terminate a session, wiping its `per_session` VFS immediately. The MCP
/// transport's HTTP `DELETE` maps to the same `wipe_now` path.
pub async fn disconnect(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> axum::response::Response {
    match state.session_manager().wipe_now(&session_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => session_state_response(error),
    }
}

pub(crate) fn session_state_response(
    error: crate::session_manager::SessionError,
) -> axum::response::Response {
    use crate::session_manager::SessionError;
    let (status, message) = match &error {
        SessionError::CleanupPending => (
            StatusCode::CONFLICT,
            "session cleanup is pending".to_owned(),
        ),
        SessionError::UnknownSession => (
            StatusCode::NOT_FOUND,
            "unknown or expired session".to_owned(),
        ),
        SessionError::Secrets(message) => (StatusCode::CONFLICT, message.clone()),
        SessionError::InvalidVfs(_)
        | SessionError::UnknownVolume(_)
        | SessionError::VolumeUnavailable(_)
        | SessionError::MountFailed { .. } => {
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
        SessionError::Storage(_) | SessionError::Io(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "session state unavailable".to_owned(),
        ),
    };
    tracing::error!(%error, "session state unavailable");
    if let Some(audit) = crate::audit::execution() {
        audit.error(crate::error::ErrorKind::RuntimeError);
    }
    (
        status,
        Json(serde_json::json!({
            "execution_id": crate::audit::execution_id(),
            "error": "session_state_unavailable",
            "message": message
        })),
    )
        .into_response()
}

fn session_header(session_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.insert(HeaderName::from_static(SESSION_HEADER), value);
    }
    headers
}

fn execution_store_failure(error: crate::blueprint::StoreError) -> axum::response::Response {
    if let Some(audit) = crate::audit::execution() {
        audit.error(crate::error::ErrorKind::RuntimeError);
    }
    let (status, Json(mut body)) = crate::blueprint::store_failure_response(error);
    if let Some(fields) = body.as_object_mut() {
        fields.insert(
            "execution_id".into(),
            serde_json::json!(crate::audit::execution_id()),
        );
    }
    (status, Json(body)).into_response()
}
