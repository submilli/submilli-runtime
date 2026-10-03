use std::collections::BTreeMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, VarBindings, resolve_harness_secrets, resolve_variables,
};
use submilli_shared::{BlueprintAuthProxy, BlueprintSecretProvider, PolicyCheck};

use crate::app::AppState;
use crate::error::{ErrorKind, ExecuteError};
use crate::runner::{self, RunOutcome};
use crate::session::LastRun;

/// Response header echoing the session id a run was recorded under, so a caller
/// can read it back via the session API. Named to match the MCP transport's
/// `MCP-Session-Id`.
pub const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub code: String,
    pub blueprint: String,
    /// Caller-supplied `${vars.NAME}` bindings for the blueprint's declared
    /// session variables. Validated against the declarations before the program
    /// runs; a missing required (or undeclared) variable rejects the request.
    #[serde(default)]
    pub variables: Option<BTreeMap<String, String>>,
    /// Trusted harness credentials for this one-shot session. Never persisted.
    #[serde(default)]
    pub secrets: Option<HarnessSecretBindings>,
}

#[derive(Debug, Serialize)]
pub struct ExecuteResponse {
    pub execution_id: String,
    pub session_id: String,
    /// `main()`'s output, verbatim: a `string` return as-is, other returns as
    /// their JSON text. The caller parses it if a structured value is expected.
    pub result: Option<String>,
    pub console: Vec<String>,
    pub error: Option<ExecuteError>,
    /// `@mcp/<server>` discovery notices (dropped/degraded tools). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discovery_warnings: Vec<String>,
}

/// What to tell a caller whose blueprint name resolved to nothing.
///
/// A name can be registered but unrunnable: a stored blueprint whose form this
/// binary no longer accepts is held out of the parsed set while its name stays
/// reserved. Reporting that as "unknown blueprint" would send the operator looking
/// for a blueprint that is sitting in the store with readable YAML, so every route
/// that resolves a name renders the reason through here — REST and MCP alike.
pub(crate) async fn blueprint_miss_message(
    state: &AppState,
    name: &str,
) -> Result<String, crate::blueprint::StoreError> {
    Ok(match state.blueprints().unusable_reason(name).await? {
        Some(reason) => reason,
        None => format!("unknown blueprint: {name}"),
    })
}

pub async fn handle(
    State(state): State<AppState>,
    request: Result<Json<ExecuteRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let req = match request {
        Ok(Json(req)) => req,
        Err(error) => {
            return (
                error.status(),
                Json(error_response(
                    "",
                    ErrorKind::InvalidRequest,
                    error.body_text(),
                )),
            )
                .into_response();
        }
    };
    // Stateless one-shot: every call gets a fresh transient session, torn down
    // once the run returns. A caller that wants state across executes (a
    // persistent `per_session` VFS, reused variable bindings) uses the session
    // API instead — `POST /v1/sessions` then `POST /v1/sessions/{id}/execute`.
    // The generated id is returned so the run's output stays readable via
    // `GET /v1/sessions/{id}/last-run`.
    let session_id = Uuid::new_v4().to_string();
    if let Some(audit) = crate::audit::execution() {
        audit.annotate(
            &req.code,
            &req.blueprint,
            None,
            &req.variables.clone().unwrap_or_default(),
        );
    }

    // Blueprint and variables are supplied inline and validated here, before the
    // shared core runs them.
    let found = match state.blueprints().get(&req.blueprint).await {
        Ok(found) => found,
        Err(error) => {
            return with_session_header(
                &session_id,
                error_response(
                    &session_id,
                    ErrorKind::RuntimeError,
                    crate::blueprint::store_failure_message(error).into(),
                ),
            )
            .into_response();
        }
    };
    let Some(blueprint) = found else {
        let message = match blueprint_miss_message(&state, &req.blueprint).await {
            Ok(message) => message,
            Err(error) => {
                return with_session_header(
                    &session_id,
                    error_response(
                        &session_id,
                        ErrorKind::RuntimeError,
                        crate::blueprint::store_failure_message(error).into(),
                    ),
                )
                .into_response();
            }
        };
        return with_session_header(
            &session_id,
            error_response(&session_id, ErrorKind::BlueprintNotFound, message),
        )
        .into_response();
    };
    let blueprint = Arc::new(blueprint);

    let supplied = req.variables.clone().unwrap_or_default();
    if let Some(audit) = crate::audit::execution() {
        audit.annotate(&req.code, &req.blueprint, Some(&blueprint), &supplied);
    }
    let variables = match resolve_variables(&blueprint.variables, &supplied) {
        Ok(resolved) => Arc::new(resolved),
        Err(err) => {
            return with_session_header(
                &session_id,
                error_response(
                    &session_id,
                    ErrorKind::InvalidRequest,
                    format!("invalid variables: {err}"),
                ),
            )
            .into_response();
        }
    };
    if let Err(error) = blueprint
        .vfs
        .resolve(&variables)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(&blueprint, &variables).map(|_| ()))
    {
        return with_session_header(
            &session_id,
            error_response(&session_id, ErrorKind::InvalidRequest, error.to_string()),
        )
        .into_response();
    }
    if let Some(audit) = crate::audit::execution() {
        audit.annotate(&req.code, &req.blueprint, Some(&blueprint), &variables);
    }
    let supplied_secrets = req.secrets.clone().unwrap_or_default();
    let harness_secrets = match resolve_harness_secrets(&blueprint.secrets, &supplied_secrets) {
        Ok(resolved) => Arc::new(resolved),
        Err(err) => {
            return with_session_header(
                &session_id,
                error_response(
                    &session_id,
                    ErrorKind::InvalidRequest,
                    format!("invalid secrets: {err}"),
                ),
            )
            .into_response();
        }
    };

    if let Err(error) = state
        .session_manager()
        .bind(
            &session_id,
            &blueprint,
            Arc::clone(&variables),
            Arc::clone(&harness_secrets),
        )
        .await
    {
        return with_session_header(
            &session_id,
            error_response(&session_id, ErrorKind::InvalidRequest, error.to_string()),
        )
        .into_response();
    }

    // The one-shot path mints a fresh session per call, so a key would have
    // nothing durable to bind to: `dispatched` is irrelevant here and any
    // `Idempotency-Key` header is ignored rather than rejected.
    let outcome = execute_core(
        &state,
        ExecuteInputs {
            session_id: &session_id,
            code: &req.code,
            blueprint_name: &req.blueprint,
            blueprint,
            variables,
            harness_secrets,
        },
    )
    .await;
    // `execute_core` registers the session so a VFS and a KV store exist for the
    // run, and a one-shot's must not outlive it: left registered it would hold
    // its share of the server-wide session-state budget until `idle_timeout`, a
    // day by default. The last-run record lives in a separate store, so the
    // teardown does not take it.
    state.session_manager().wipe_now(&session_id).await;
    with_session_header(&session_id, outcome.response).into_response()
}

/// Already-resolved inputs to one execution, shared by the one-shot
/// `POST /v1/execute` and the session-scoped `POST /v1/sessions/{id}/execute`
/// handlers. Each handler resolves the blueprint and variables its own way (the
/// one-shot reads them from the request body; the session path reads them from
/// the bound session) before handing off to [`execute_core`].
pub(crate) struct ExecuteInputs<'a> {
    pub session_id: &'a str,
    pub code: &'a str,
    /// The blueprint's name, used to key MCP catalog / package resolution and
    /// the outbound MCP transport.
    pub blueprint_name: &'a str,
    pub blueprint: Arc<Blueprint>,
    pub variables: Arc<VarBindings>,
    pub harness_secrets: Arc<HarnessSecretBindings>,
}

/// What one execution produced, plus whether the program actually reached the
/// runner.
///
/// The distinction only matters to callers that record outcomes. A failure
/// *before* dispatch — session init, VFS init — means nothing ran and no effects
/// landed, so caching it would pin a transient infrastructure failure to that
/// key forever: a retry after a full disk would replay the failure rather than
/// succeed. Everything from dispatch onward is a real result, including compile
/// errors, traps, and timeouts.
pub(crate) struct ExecuteOutcome {
    pub response: ExecuteResponse,
    pub dispatched: bool,
}

impl ExecuteOutcome {
    fn undispatched(response: ExecuteResponse) -> Self {
        Self {
            response,
            dispatched: false,
        }
    }

    fn dispatched(response: ExecuteResponse) -> Self {
        Self {
            response,
            dispatched: true,
        }
    }
}

/// Run one program against a session and record its last-run. Registers the
/// session id (idempotent), builds the per-session VFS + HTTP client + host
/// services, resolves imports, runs, and touches the session's idle timer.
pub(crate) async fn execute_core(state: &AppState, inputs: ExecuteInputs<'_>) -> ExecuteOutcome {
    if let Some(audit) = crate::audit::execution() {
        audit.begin();
    }
    let ExecuteInputs {
        session_id,
        code,
        blueprint_name,
        blueprint,
        variables,
        harness_secrets,
    } = inputs;

    let parsed = match runner::parse(code) {
        Ok(parsed) => parsed,
        Err(error) => {
            return ExecuteOutcome::undispatched(error_response(
                session_id,
                ErrorKind::RuntimeError,
                error.to_string(),
            ));
        }
    };
    let script_imports = match parsed.imports() {
        Ok(imports) => imports,
        Err(message) => {
            return ExecuteOutcome::undispatched(error_response(
                session_id,
                ErrorKind::RuntimeError,
                message,
            ));
        }
    };

    let manager = state.session_manager();
    if let Err(err) = manager.ensure(session_id, &blueprint).await {
        return ExecuteOutcome::undispatched(error_response(
            session_id,
            ErrorKind::RuntimeError,
            format!("internal: session init failed: {err}"),
        ));
    }
    // Mark activity at entry as well as at exit. The idle reaper is wall-clock
    // and has no notion of an execution in flight, so a session that was
    // already nearly idle when this call arrived would otherwise be reaped
    // moments into the run — taking its idempotency reservation with it. This
    // buys the program a full idle window rather than whatever was left of one;
    // it does not make the run un-reapable, and an execution that outlasts
    // `idle_timeout` on its own is still collected mid-flight. The write is
    // debounced (`PERSIST_INTERVAL`), so this costs nothing per call.
    manager.touch(session_id).await;
    let (vfs, vfs_info) = match manager
        .vfs_for_execute_with_variables(session_id, &blueprint, &variables)
        .await
    {
        Ok(pair) => pair,
        Err(err) => {
            return ExecuteOutcome::undispatched(error_response(
                session_id,
                ErrorKind::RuntimeError,
                format!("internal: vfs init failed: {err}"),
            ));
        }
    };

    let execution_audit = crate::audit::execution();
    let network_policy = execution_audit.as_ref().map_or_else(
        || state.network_policy().clone(),
        |audit| audit.network_policy(state.network_policy()),
    );
    let http_client = manager.http_client(session_id);
    let mcp_transport = Arc::new(
        submilli_shared::mcp::transport::StreamableHttpTransport::new(
            blueprint_name.to_string(),
            Arc::clone(&blueprint),
            state.oauth_token_manager().cloned(),
            state.secret_store().cloned(),
            Arc::clone(&network_policy),
        )
        .with_harness_secrets(Arc::clone(&harness_secrets)),
    );
    let policy: Arc<dyn interpreter::runtime::SecurityCheck> = Arc::new(
        PolicyCheck::with_variables(Arc::clone(&blueprint), Arc::clone(&variables)),
    );
    let security_check = execution_audit.as_ref().map_or_else(
        || policy.clone(),
        |execution| {
            Arc::new(crate::audit::AuditedPolicy {
                policy: policy.clone(),
                execution: execution.clone(),
            }) as Arc<dyn interpreter::runtime::SecurityCheck>
        },
    );
    let services = runner::HostServices {
        audit: execution_audit,
        git: submilli_shared::resolve_git(&blueprint, &variables)
            .map_err(|error| error.to_string()),
        auth_proxy: Arc::new(BlueprintAuthProxy::with_harness(
            Arc::clone(&blueprint),
            state.secret_store().cloned(),
            Arc::clone(&harness_secrets),
        )),
        secret_provider: Arc::new(BlueprintSecretProvider::with_harness(
            Arc::clone(&blueprint),
            state.secret_store().cloned(),
            Arc::clone(&harness_secrets),
        )),
        security_check,
        http_client,
        mcp_transport,
        session_kv: manager.session_kv_for_execute(session_id),
        llm_provider: state.llm_provider_for(&blueprint, &harness_secrets, &network_policy),
        llm_budget: Some(manager.llm_budget_for_execute()),
    };
    let mcp_catalog = state
        .mcp_catalog_for_imports(
            blueprint_name,
            &blueprint,
            &script_imports.mcp_servers,
            &harness_secrets,
            &network_policy,
        )
        .await;
    let packages =
        match state.prepared_packages_for_imports(blueprint_name, &blueprint, &script_imports) {
            Ok(packages) => packages,
            Err(err) => {
                return ExecuteOutcome::undispatched(error_response(
                    session_id,
                    ErrorKind::PackageResolution,
                    err.to_string(),
                ));
            }
        };
    let outcome = runner::run(
        code,
        parsed,
        runner::RunnerRuntime {
            blueprint: blueprint_name,
            session: session_id,
            engine: state.engine(),
            base_linker: state.base_linker(),
            config: state.runtime(),
        },
        vfs,
        vfs_info,
        services,
        runner::RunnerImports {
            packages: &packages,
            mcps: &mcp_catalog,
        },
    )
    .await;
    manager.touch(session_id).await;

    let console_lines = split_console(&outcome.console_raw);
    let response = into_response(session_id, &outcome, &console_lines);

    // The success response omits console output; the session keeps the full
    // capture so `/v1/last-run/{id}` can still return it.
    state
        .sessions()
        .record(
            session_id,
            LastRun {
                result: response.result.clone(),
                console: console_lines,
                error: response.error.clone(),
            },
        )
        .await;

    ExecuteOutcome::dispatched(response)
}

pub(crate) fn with_session_header(
    session_id: &str,
    body: ExecuteResponse,
) -> (HeaderMap, Json<ExecuteResponse>) {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.insert(HeaderName::from_static(SESSION_HEADER), value);
    }
    (headers, Json(body))
}

fn error_response(session_id: &str, kind: ErrorKind, message: String) -> ExecuteResponse {
    if let Some(audit) = crate::audit::execution() {
        audit.error(kind);
    }
    ExecuteResponse {
        execution_id: crate::audit::execution_id(),
        session_id: session_id.to_string(),
        result: None,
        console: Vec::new(),
        error: Some(ExecuteError {
            kind,
            message,
            diagnostics: Vec::new(),
        }),
        discovery_warnings: Vec::new(),
    }
}

fn into_response(
    session_id: &str,
    outcome: &RunOutcome,
    console_lines: &[String],
) -> ExecuteResponse {
    let (result, console, error) = outcome_to_parts(outcome, console_lines);
    ExecuteResponse {
        execution_id: crate::audit::execution_id(),
        session_id: session_id.to_string(),
        result,
        console,
        error,
        discovery_warnings: outcome.discovery_warnings.clone(),
    }
}

/// Map a `RunOutcome` to the transport-agnostic `{ result, console, error }`
/// triple shared by the REST handler and the MCP tool. Console output is
/// suppressed on success (the full capture lives in the session's last-run).
pub(crate) fn outcome_to_parts(
    outcome: &RunOutcome,
    console_lines: &[String],
) -> (Option<String>, Vec<String>, Option<ExecuteError>) {
    if let Some(error) = &outcome.error {
        return (None, console_lines.to_vec(), Some(error.clone()));
    }
    // `main`'s output is already a string (codegen's output shim); pass it through
    // verbatim — the caller decides whether to parse it as JSON.
    (outcome.value.clone(), Vec::new(), None)
}

pub(crate) fn split_console(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = raw
        .split('\n')
        .map(std::string::ToString::to_string)
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}
