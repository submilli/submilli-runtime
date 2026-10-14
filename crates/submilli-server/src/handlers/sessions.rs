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
use tokio_util::sync::CancellationToken;
use tokio_util::task::task_tracker::TaskTrackerToken;

use submilli_blueprint::{
    HarnessSecretBindings, required_harness_secrets, resolve_harness_secrets, resolve_variables,
};

use crate::app::{AppState, BlueprintForRun};
use crate::domain::idempotent_request::RecordedOutcome;
use crate::domain::tools::{DeclaredTool, SessionTools};
use crate::handlers::execute::{self, ExecuteInputs, SESSION_HEADER, blueprint_miss_message};
use crate::idempotency::{Refusal, Reservation, ReservationGuard};
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
    /// The application tools programs in this session may call as `@tools`,
    /// fixed for the session's life. The harness answers each call.
    #[serde(default)]
    pub tools: Option<Vec<DeclaredTool>>,
}

#[derive(Debug, Serialize)]
pub struct CreateResponse {
    pub session_id: String,
    /// What programs will not see of the declared tools, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
        req.tools.unwrap_or_default(),
    )
    .await;
    match started {
        Ok(Started {
            session_id,
            warnings,
        }) => {
            let body = CreateResponse {
                session_id: session_id.clone(),
                warnings,
            };
            (session_header(&session_id), Json(body)).into_response()
        }
        Err(error) => start_refusal_response(error),
    }
}

/// A session [`start`] created.
pub(crate) struct Started {
    pub session_id: String,
    /// What programs will not see of the declared tools, and why.
    pub warnings: Vec<String>,
}

/// Creates a session bound to `blueprint_name`, its resolved variables, its harness
/// secrets, and its application tools, and returns its id. Everything is validated
/// before the session exists: defaults fill, and a missing-required or undeclared name
/// refuses the request.
pub(crate) async fn start(
    state: &AppState,
    blueprint_name: &str,
    supplied: &BTreeMap<String, String>,
    supplied_secrets: &HarnessSecretBindings,
    tools: Vec<DeclaredTool>,
) -> Result<Started, SessionStartError> {
    let tools = SessionTools::new(tools)
        .map_err(|error| SessionStartError::InvalidTools(error.to_string()))?;
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

    let warnings = tools.as_ref().map_or_else(Vec::new, |tools| {
        let mut warnings = crate::app_tools::declaration(tools).1;
        if !exposes_tools(&blueprint) {
            warnings.push(format!(
                "{}: blueprint `{}` grants no `{}`, so programs cannot see these tools",
                interpreter::runtime::TOOLS_PACKAGE,
                blueprint.name,
                interpreter::runtime::TOOLS_CAPABILITY,
            ));
        }
        warnings
    });
    let session_id = state
        .session_manager()
        .create_with_tools(&blueprint, variables, secrets, tools)
        .await
        .map_err(SessionStartError::Session)?;
    Ok(Started {
        session_id,
        warnings,
    })
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
        | SessionStartError::InvalidSecrets(_)
        | SessionStartError::InvalidTools(_) => (
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
    // A session with application tools runs its programs detached from the
    // request, so a run can wait on the harness across requests.
    let tools = match session_tools(&state, &session_id, &bound).await {
        Ok(tools) => tools,
        Err(error) => return session_state_response(error),
    };
    // Every session-level refusal above happens before any reservation exists,
    // so a bad key against an unknown session reports the unknown session and
    // the ledger stays untouched. This is the last point before the
    // program can run.
    let key = match idempotency_key(&headers) {
        None => None,
        Some(Err(refusal)) => return refusal_response(&session_id, &refusal),
        Some(Ok(key)) => Some(key),
    };
    let guard = match &key {
        None => None,
        Some(key) => match reserve(&state, &session_id, key, &req.code).await {
            Reserved::Run(guard) => Some(guard),
            Reserved::Answered(response) => return response,
        },
    };
    if let Some(tools) = tools {
        return run_detached(&state, session_id, bound, req.code, key, guard, tools).await;
    }

    let outcome = run(
        &state,
        &session_id,
        bound,
        &req.code,
        key.as_deref(),
        crate::audit::execution(),
        None,
    )
    .await;
    // Serialize once and hand the same bytes to the ledger and the caller, so a
    // replay cannot drift from what the original client received.
    let finished = crate::handlers::runs::finished_body(&outcome.response);
    if let Some(guard) = guard {
        let outcome_recordable = true;
        settle(guard, outcome.dispatched, &finished, outcome_recordable).await;
    }
    stored_body_response(&session_id, StatusCode::OK, finished.body)
}

/// Settle the key a run held: record `finished` when the program ran and its
/// outcome may be recorded, release the key when nothing reached the runner
/// (caching a transient failure would pin it to the key forever), and
/// otherwise leave the key indeterminate by dropping the guard armed, since the
/// program ran but what it did cannot be told.
async fn settle(
    guard: ReservationGuard,
    dispatched: bool,
    finished: &crate::handlers::runs::FinishedBody,
    outcome_recordable: bool,
) {
    if !dispatched {
        guard.release().await;
    } else if finished.recordable && outcome_recordable {
        guard
            .complete(StatusCode::OK.as_u16(), finished.body.clone())
            .await;
    } else {
        drop(guard);
    }
}

/// What reserving an idempotency key came to.
enum Reserved {
    /// The program runs under this reservation.
    Run(ReservationGuard),
    /// The request is answered without running: a replay of the recorded
    /// outcome, the current state of a run still going, or a refusal.
    Answered(axum::response::Response),
}

/// Reserve `key` for a run, or answer the request without one.
async fn reserve(state: &AppState, session_id: &str, key: &str, code: &str) -> Reserved {
    // A run that is still going may have finished by the time it is looked up;
    // its outcome is then recorded, and a second look finds it.
    for _ in 0..2 {
        match state.idempotency().reserve(session_id, key, code).await {
            Reservation::Proceed(guard) => return Reserved::Run(guard),
            Reservation::Refused(refusal) => {
                return Reserved::Answered(refusal_response(session_id, &refusal));
            }
            Reservation::Replay(outcome) => {
                return Reserved::Answered(replay_response(state, session_id, key, &outcome).await);
            }
            Reservation::Attached(run_id) => {
                let Some(entry) = state.session_manager().runs().get(&run_id) else {
                    continue;
                };
                if let Some(refused) = note_duplicate(state, session_id).await {
                    return Reserved::Answered(refused);
                }
                let settled = entry.settled().await;
                return Reserved::Answered(crate::handlers::runs::state_response(&entry, settled));
            }
        }
    }
    Reserved::Answered(refusal_response(session_id, &Refusal::InProgress))
}

async fn replay_response(
    state: &AppState,
    session_id: &str,
    key: &str,
    outcome: &RecordedOutcome,
) -> axum::response::Response {
    if let Some(recorder) = state.run_recorder() {
        recorder.retried(crate::record::RetryLink {
            session_id: session_id.to_owned(),
            idempotency_key: key.to_owned(),
            original_execution_id: serde_json::from_str::<serde_json::Value>(&outcome.body)
                .ok()
                .and_then(|body| body.get("execution_id")?.as_str().map(str::to_owned)),
        });
    }
    if let Some(refused) = note_duplicate(state, session_id).await {
        return refused;
    }
    recorded_response(session_id, outcome)
}

/// Note a duplicate request answered without running: it is a replay in the
/// audit trail, and session activity, so a client retrying does not have its
/// session reaped underneath it (`execute_core` normally does this, and a
/// duplicate never reaches it). Returns the refusal to send when the session
/// can no longer be used.
async fn note_duplicate(state: &AppState, session_id: &str) -> Option<axum::response::Response> {
    if let Some(audit) = crate::audit::execution() {
        audit.replay();
    }
    state
        .session_manager()
        .touch(session_id)
        .await
        .err()
        .map(session_state_response)
}

/// The application tools a program in `session_id` may call: the session's
/// catalog, when its blueprint lets programs see `@tools`.
async fn session_tools(
    state: &AppState,
    session_id: &str,
    bound: &BoundSession,
) -> Result<Option<SessionTools>, crate::session_manager::SessionError> {
    if !exposes_tools(&bound.found.blueprint) {
        return Ok(None);
    }
    state.session_manager().tools(session_id).await
}

/// Runs `code` in its own task, so it can outlive this request while it waits on
/// the harness, and answers with the run's first settled state: suspended at a
/// tool call, or finished. Everything that follows a run (its audit record, its
/// last-run, its idempotency outcome) happens in that task, exactly once,
/// whoever is listening.
async fn run_detached(
    state: &AppState,
    session_id: String,
    bound: BoundSession,
    code: String,
    key: Option<String>,
    guard: Option<ReservationGuard>,
    tools: SessionTools,
) -> axum::response::Response {
    let shutdown = state.graceful_shutdown();
    let Some(admitted) = shutdown.admit() else {
        if let Some(guard) = guard {
            guard.release().await;
        }
        return (StatusCode::SERVICE_UNAVAILABLE, "server is shutting down").into_response();
    };
    // The run's id is its execution id, so it needs an audit to name it even
    // where the request carries none.
    let audit = crate::audit::execution().unwrap_or_else(|| {
        crate::audit::ExecutionAudit::new(
            state.audit().clone(),
            "unauthenticated",
            "http",
            Some(&session_id),
        )
    });
    let registry = Arc::clone(state.session_manager().runs());
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let entry = registry.insert(&audit.id, &session_id, cancel);
    if let Some(guard) = &guard {
        guard.attach_run(&entry.id);
    }
    let tools = run_tools(state, &entry, &session_id, &bound, &tools, cancelled);
    // The record ends with the run, not with this request.
    audit.hand_off();
    let supervised = Supervised {
        state: state.clone(),
        entry: Arc::clone(&entry),
        registry,
        session_id,
        bound,
        code,
        key,
        guard,
        audit,
        tools,
    };
    supervised.spawn(admitted, shutdown.draining());
    // A client going away does not end this handler (`graceful_shutdown::run`
    // owns it), so this fires only when the server stops the request outright:
    // the run is then abandoned before anyone learned of a call to answer.
    let mut abandon = CancelOnDrop(Some(Arc::clone(&entry)));
    let settled = entry.settled().await;
    abandon.0 = None;
    crate::handlers::runs::state_response(&entry, settled)
}

/// What [`run_detached`]'s task owns: the run, and everything that follows it.
struct Supervised {
    state: AppState,
    entry: Arc<crate::runs::RunEntry>,
    registry: Arc<crate::runs::RunRegistry>,
    session_id: String,
    bound: BoundSession,
    code: String,
    key: Option<String>,
    guard: Option<ReservationGuard>,
    audit: Arc<crate::audit::ExecutionAudit>,
    tools: execute::RunTools,
}

impl Supervised {
    /// Supervise the run in its own task, which holds `admitted` until the run
    /// has ended. Once the server drains, a run waiting on the harness stops;
    /// one that is working finishes within the shutdown grace.
    fn spawn(self, admitted: TaskTrackerToken, draining: CancellationToken) {
        let entry = Arc::clone(&self.entry);
        tokio::spawn(async move {
            let _admitted = admitted;
            let supervise = self.supervise();
            tokio::pin!(supervise);
            tokio::select! {
                biased;
                () = &mut supervise => {}
                () = draining.cancelled() => {
                    entry.stop_waiting();
                    supervise.await;
                }
            }
        });
    }

    /// Run the program, then end its record, settle its idempotency key, and
    /// publish its result.
    async fn supervise(self) {
        let outcome = run(
            &self.state,
            &self.session_id,
            self.bound,
            &self.code,
            self.key.as_deref(),
            Some(Arc::clone(&self.audit)),
            Some(self.tools),
        )
        .await;
        // A run that never reached the runner has no one else to end its record.
        self.audit.finish(outcome.response.error.is_none());
        let finished = crate::handlers::runs::finished_body(&outcome.response);
        if let Some(guard) = self.guard {
            // A deleted session's keys are gone, and writing one back would
            // revive it. A run the server stopped did not end on its own.
            let outcome_recordable = self.entry.outcome_recordable();
            settle(guard, outcome.dispatched, &finished, outcome_recordable).await;
        }
        self.entry.finish(Arc::from(finished.body));
        self.registry.retire(&self.entry);
    }
}

/// The `@tools` a detached run calls: the session's catalog as a package, and a
/// provider that suspends the run at each call.
fn run_tools(
    state: &AppState,
    entry: &Arc<crate::runs::RunEntry>,
    session_id: &str,
    bound: &BoundSession,
    tools: &SessionTools,
    cancelled: tokio::sync::oneshot::Receiver<()>,
) -> execute::RunTools {
    let (declaration, warnings) = crate::app_tools::declaration(tools);
    execute::RunTools {
        package: Arc::new(crate::runner::ToolsPackage {
            declaration,
            warnings,
        }),
        provider: Arc::new(crate::runs::SuspendingToolProvider {
            entry: Arc::clone(entry),
            timeout: bound.found.blueprint.tool_call_timeout,
            touch: session_touch(state, session_id),
        }),
        cancel: Some(cancelled),
    }
}

/// Marks `session_id` active, for a run that suspends in it.
fn session_touch(
    state: &AppState,
    session_id: &str,
) -> Arc<dyn Fn() -> crate::runs::Touch + Send + Sync> {
    let manager = Arc::clone(state.session_manager());
    let session_id = session_id.to_owned();
    Arc::new(move || {
        let manager = Arc::clone(&manager);
        let session_id = session_id.clone();
        Box::pin(async move {
            if let Err(error) = manager.touch(&session_id).await {
                tracing::warn!(%error, "session activity at a tool call did not commit");
            }
        })
    })
}

/// Cancels a run whose first answer nobody waited for.
struct CancelOnDrop(Option<Arc<crate::runs::RunEntry>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(entry) = self.0.take() {
            entry.cancel_unrecorded();
        }
    }
}

/// Whether programs under `blueprint` may see a session's `@tools`.
fn exposes_tools(blueprint: &submilli_blueprint::Blueprint) -> bool {
    submilli_shared::library_visibility::LibraryVisibility::for_blueprint(blueprint).tools()
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
/// `tools` are the session's application tools, for a run that may suspend.
pub(crate) async fn run(
    state: &AppState,
    session_id: &str,
    bound: BoundSession,
    code: &str,
    idempotency_key: Option<&str>,
    audit: Option<Arc<crate::audit::ExecutionAudit>>,
    tools: Option<execute::RunTools>,
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
            tools,
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
