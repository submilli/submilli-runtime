use std::collections::{BTreeMap, BTreeSet};
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

use crate::app::{AppState, BlueprintForRun};
use crate::error::{ErrorKind, ExecuteError};
use crate::record::{RunEntry, TestWorld};
use interpreter::runtime::{Vfs, VfsInfo};

use crate::runner::{self, RunOutcome};
use crate::session::LastRun;
use crate::session_manager::SessionError;

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
    let (session_id, response) =
        one_shot(&state, req, crate::audit::execution(), RunEntry::Http).await;
    with_session_header(&session_id, response).into_response()
}

/// Runs one program in a fresh session that is torn down once it returns, the
/// `POST /v1/execute` shape. Returns the session id and the response. `run_entry` is how the
/// run is recorded: [`RunEntry::Http`] for the endpoint, [`RunEntry::Program`] for an
/// in-process caller.
pub(crate) async fn one_shot(
    state: &AppState,
    req: ExecuteRequest,
    audit: Option<Arc<crate::audit::ExecutionAudit>>,
    run_entry: RunEntry,
) -> (String, ExecuteResponse) {
    one_shot_with(state, req, audit, run_entry, None).await
}

/// [`one_shot`] as a test run when `test` is given: the run's outside connectors and local
/// state come from it, and it is recorded as a test of its source.
pub(crate) async fn one_shot_with(
    state: &AppState,
    req: ExecuteRequest,
    audit: Option<Arc<crate::audit::ExecutionAudit>>,
    run_entry: RunEntry,
    test: Option<TestWorld>,
) -> (String, ExecuteResponse) {
    // Stateless one-shot: every call gets a fresh transient session, torn down
    // once the run returns. A caller that wants state across executes (a
    // persistent `per_session` VFS, reused variable bindings) uses the session
    // API instead — `POST /v1/sessions` then `POST /v1/sessions/{id}/execute`.
    // The generated id is returned so the run's output stays readable via
    // `GET /v1/sessions/{id}/last-run`.
    let session_id = Uuid::new_v4().to_string();
    let failed = |kind: ErrorKind, message: String| {
        (
            session_id.clone(),
            failure_response(audit.as_deref(), &session_id, kind, message),
        )
    };
    if let Some(audit) = &audit {
        audit.annotate(
            &req.code,
            &req.blueprint,
            None,
            &req.variables.clone().unwrap_or_default(),
        );
    }

    // Blueprint and variables are supplied inline and validated here, before the
    // shared core runs them.
    // A test run was prepared under a blueprint it already looked up; it runs under
    // that one.
    let found = match test.as_ref().map(TestWorld::blueprint) {
        Some(found) => Ok(Some(found.clone())),
        None => state.blueprint_for_run(&req.blueprint).await,
    };
    let found = match found {
        Ok(found) => found,
        Err(error) => {
            return failed(
                ErrorKind::RuntimeError,
                crate::blueprint::store_failure_message(error).into(),
            );
        }
    };
    let Some(BlueprintForRun {
        blueprint,
        version_tag,
    }) = found
    else {
        return match blueprint_miss_message(state, &req.blueprint).await {
            Ok(message) => failed(ErrorKind::BlueprintNotFound, message),
            Err(error) => failed(
                ErrorKind::RuntimeError,
                crate::blueprint::store_failure_message(error).into(),
            ),
        };
    };
    let blueprint = Arc::new(blueprint);

    let supplied = req.variables.clone().unwrap_or_default();
    if let Some(audit) = &audit {
        audit.annotate(&req.code, &req.blueprint, Some(&blueprint), &supplied);
    }
    let variables = match resolve_variables(&blueprint.variables, &supplied) {
        Ok(resolved) => Arc::new(resolved),
        Err(err) => {
            return failed(
                ErrorKind::InvalidRequest,
                format!("invalid variables: {err}"),
            );
        }
    };
    if let Err(error) = blueprint
        .vfs
        .resolve(&variables)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(&blueprint, &variables).map(|_| ()))
    {
        return failed(ErrorKind::InvalidRequest, error.to_string());
    }
    if let Some(audit) = &audit {
        audit.annotate(&req.code, &req.blueprint, Some(&blueprint), &variables);
    }
    let supplied_secrets = req.secrets.clone().unwrap_or_default();
    let harness_secrets = match resolve_harness_secrets(&blueprint.secrets, &supplied_secrets) {
        Ok(resolved) => Arc::new(resolved),
        Err(err) => {
            return failed(ErrorKind::InvalidRequest, format!("invalid secrets: {err}"));
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
        let (kind, message) = if let SessionError::InvalidVfs(_) = error {
            (ErrorKind::InvalidRequest, error.to_string())
        } else {
            tracing::error!(%error, session = %session_id, "one-shot session bind failed");
            (ErrorKind::RuntimeError, error.to_string())
        };
        return failed(kind, message);
    }

    // The one-shot path mints a fresh session per call, so a key would have
    // nothing durable to bind to: `dispatched` is irrelevant here and any
    // `Idempotency-Key` header is ignored rather than rejected.
    let outcome = execute_core_with(
        state,
        ExecuteInputs {
            session_id: &session_id,
            code: &req.code,
            blueprint_name: &req.blueprint,
            blueprint,
            version_tag,
            variables,
            harness_secrets,
            audit,
            vfs_source: VfsSource::Rest,
            run_entry,
            client: None,
            tool_call_id: None,
            idempotency_key: None,
        },
        test,
    )
    .await;
    // `execute_core` registers the session so a VFS and a KV store exist for the
    // run, and a one-shot's must not outlive it: left registered it would hold
    // its share of the server-wide session-state budget until `idle_timeout`, a
    // day by default. The last-run record lives in a separate store, so the
    // teardown does not take it.
    state.session_manager().wipe_now(&session_id).await;
    (session_id, outcome.response)
}

/// Already-resolved inputs to one execution, shared by the one-shot
/// `POST /v1/execute`, the session-scoped `POST /v1/sessions/{id}/execute`, and the MCP
/// execute tool. Each caller resolves the blueprint and variables its own way (the
/// one-shot reads them from the request body; the session path reads them from
/// the bound session) before handing off to [`execute_core`].
pub(crate) struct ExecuteInputs<'a> {
    /// The session the run executes against; empty for an MCP request that has none.
    pub session_id: &'a str,
    /// The program's source.
    pub code: &'a str,
    /// The blueprint's name, used to key MCP catalog / package resolution and
    /// the outbound MCP transport.
    pub blueprint_name: &'a str,
    /// The blueprint the run is decided under.
    pub blueprint: Arc<Blueprint>,
    /// The version tag read with `blueprint`, in the same lookup; `None` when it was
    /// registered without one.
    pub version_tag: Option<String>,
    /// The validated `${vars.NAME}` bindings.
    pub variables: Arc<VarBindings>,
    /// Trusted harness credentials for this run alone.
    pub harness_secrets: Arc<HarnessSecretBindings>,
    /// The execution's audit record. REST handlers take it from the request's
    /// task-local; MCP creates its own per tool call.
    pub audit: Option<Arc<crate::audit::ExecutionAudit>>,
    /// Who opens the run's VFS.
    pub vfs_source: VfsSource,
    /// How the run is recorded, when it is.
    pub run_entry: RunEntry,
    /// The MCP client's name, for the run's record.
    pub client: Option<String>,
    /// The MCP client's id for the tool call, for the run's record.
    pub tool_call_id: Option<String>,
    /// The session API's `Idempotency-Key`, for the run's record.
    pub idempotency_key: Option<&'a str>,
}

/// Who opens a run's VFS, which also sets how the core audits a program that fails to
/// parse.
pub(crate) enum VfsSource {
    /// REST: the core registers the session and opens its VFS.
    Rest,
    /// MCP: the tool opened the VFS under its own file-area rules, ahead of the
    /// core. A program that fails to parse is audited as a compile error, as the
    /// MCP tool always has.
    Mcp { vfs: Vfs, vfs_info: VfsInfo },
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

/// Runs one program: starts its recording (when the server records runs), registers it
/// to be cancelled, prepares and runs it, and reports to its recorder what the caller
/// got. See [`prepare_and_run`] for the work itself.
pub(crate) async fn execute_core(state: &AppState, inputs: ExecuteInputs<'_>) -> ExecuteOutcome {
    execute_core_with(state, inputs, None).await
}

/// [`execute_core`] as a test run when `test` is given.
async fn execute_core_with(
    state: &AppState,
    inputs: ExecuteInputs<'_>,
    test: Option<TestWorld>,
) -> ExecuteOutcome {
    let recording = crate::record::Recording::start_tapped(
        state,
        || run_start(&inputs, test.as_ref()),
        test.as_ref().map(TestWorld::tap),
    );
    // A test run is reported from its own record, so one the recorder declined is refused
    // before it can have any effect.
    if let Some(test) = &test
        && recording.is_none()
    {
        test.tap().declined();
        return ExecuteOutcome::undispatched(failure_response(
            inputs.audit.as_deref(),
            inputs.session_id,
            ErrorKind::RuntimeError,
            crate::record::TestError::NoRecorder.to_string(),
        ));
    }
    // Only a recorded run can be cancelled from outside (the registry exists for a stop
    // control over recorded runs), so an unrecorded run pays for no registration. The
    // guard keeps the entry until this function returns, which is after the run has
    // ended, so `cancel_run` finds the run for exactly as long as it is in flight.
    let (_registered, cancel_requested) = match (&recording, &inputs.audit) {
        (Some(_), Some(audit)) => {
            let (registered, cancel_requested) = state.register_run(&audit.id);
            (Some(registered), Some(cancel_requested))
        }
        _ => (None, None),
    };
    let outcome = prepare_and_run(state, inputs, recording.clone(), cancel_requested, test).await;
    if let Some(recording) = &recording {
        match (&outcome.response.error, outcome.dispatched) {
            // The runner finishes a dispatched run itself; one that never got there is
            // finished here.
            (Some(error), false) => recording.undispatched(error),
            _ => recording.returned(returned_bytes(&outcome.response)),
        }
    }
    outcome
}

/// The run as its recorder first sees it.
fn run_start(inputs: &ExecuteInputs<'_>, test: Option<&TestWorld>) -> crate::record::RunStart {
    let audit = inputs.audit.as_deref();
    crate::record::RunStart {
        execution_id: execution_id_of(audit),
        label: audit.map_or_else(
            || "unauthenticated".to_owned(),
            |audit| audit.principal.clone(),
        ),
        entry: inputs.run_entry.clone(),
        test_of: test.map(|test| test.source_run().to_owned()),
        client: inputs.client.clone(),
        tool_call_id: inputs.tool_call_id.clone(),
        session_id: (!inputs.session_id.is_empty()).then(|| inputs.session_id.to_owned()),
        idempotency_key: inputs.idempotency_key.map(str::to_owned),
        blueprint_name: inputs.blueprint_name.to_owned(),
        blueprint: Arc::clone(&inputs.blueprint),
        blueprint_hash: crate::audit::blueprint_hash(&inputs.blueprint),
        blueprint_version: inputs
            .version_tag
            .clone()
            .or_else(|| crate::audit::blueprint_hash(&inputs.blueprint)),
        variables: Arc::clone(&inputs.variables),
        harness_secrets: Arc::clone(&inputs.harness_secrets),
        code: Some(Arc::from(inputs.code)),
    }
}

/// The size, as JSON, of what the caller received: the result, error, and console
/// output both transports return.
fn returned_bytes(response: &ExecuteResponse) -> u64 {
    #[derive(Serialize)]
    struct Returned<'a> {
        result: &'a Option<String>,
        console: &'a [String],
        error: &'a Option<ExecuteError>,
    }
    serde_json::to_vec(&Returned {
        result: &response.result,
        console: &response.console,
        error: &response.error,
    })
    .map_or(0, |bytes| bytes.len() as u64)
}

/// Runs one program against a session and records its last-run. Registers the session id
/// (idempotent), builds the per-session VFS + HTTP client + host services, resolves
/// imports, runs, and touches the session's idle timer.
async fn prepare_and_run(
    state: &AppState,
    inputs: ExecuteInputs<'_>,
    recording: Option<crate::record::Recording>,
    cancel_requested: Option<tokio::sync::oneshot::Receiver<()>>,
    test: Option<TestWorld>,
) -> ExecuteOutcome {
    let ExecuteInputs {
        session_id,
        code,
        blueprint_name,
        blueprint,
        version_tag: _,
        variables,
        harness_secrets,
        audit: execution_audit,
        vfs_source,
        run_entry: _,
        client: _,
        tool_call_id: _,
        idempotency_key: _,
    } = inputs;
    if let Some(audit) = &execution_audit {
        audit.begin();
    }
    let audit = execution_audit.as_deref();
    let fail = |kind: ErrorKind, message: String| {
        ExecuteOutcome::undispatched(failure_response(audit, session_id, kind, message))
    };
    let parse_audit_kind = match vfs_source {
        VfsSource::Rest => ErrorKind::RuntimeError,
        VfsSource::Mcp { .. } => ErrorKind::CompileError,
    };
    let fail_to_parse = |message: String| {
        if let Some(audit) = audit {
            audit.error(parse_audit_kind);
        }
        ExecuteOutcome::undispatched(response_for_error(
            audit,
            session_id,
            ErrorKind::RuntimeError,
            message,
        ))
    };

    let parsed = match runner::parse(code) {
        Ok(parsed) => parsed,
        Err(error) => return fail_to_parse(error.to_string()),
    };
    let script_imports = match parsed.imports() {
        Ok(imports) => imports,
        Err(message) => return fail_to_parse(message),
    };
    // Every entry point reaches here before anything is compiled, so the embedder's
    // check runs for every run, whoever sent it.
    if let Some(hook) = state.pre_execute() {
        let packages: BTreeSet<String> = script_imports
            .registry_packages
            .iter()
            .filter(|package| blueprint.packages.contains(*package))
            .cloned()
            .collect();
        let checked = hook
            .before_execute(crate::config::PreExecute {
                state,
                blueprint_name,
                blueprint: &blueprint,
                packages: &packages,
            })
            .await;
        if let Err(refusal) = checked {
            return fail(ErrorKind::PackageResolution, refusal.message);
        }
    }

    let network_policy = execution_audit.as_ref().map_or_else(
        || state.network_policy().clone(),
        |audit| audit.network_policy(state.network_policy()),
    );
    let llm_provider = match state.llm_provider_for(&blueprint, &harness_secrets, &network_policy) {
        Ok(provider) => provider,
        Err(error) => {
            tracing::error!(error = ?error, "LLM dispatch initialization failed");
            return fail(ErrorKind::RuntimeError, error.to_string());
        }
    };

    let embedding_provider =
        match state.embedding_provider_for(&blueprint, &harness_secrets, &network_policy) {
            Ok(provider) => provider,
            Err(error) => {
                tracing::error!(error = ?error, "embedding dispatch initialization failed");
                return fail(ErrorKind::RuntimeError, error.to_string());
            }
        };

    let context = WorldContext {
        state,
        session_id,
        blueprint_name,
        blueprint: &blueprint,
        variables: &variables,
        harness_secrets: &harness_secrets,
        network_policy: &network_policy,
        llm_provider,
        embedding_provider,
        cancel_requested,
    };
    // The one place a test run's world replaces the run's own.
    let world = match test {
        Some(test) => test.into_world(context).await,
        None => live_world(context, vfs_source, &script_imports.mcp_servers).await,
    };
    let RunWorld {
        mcp_catalog,
        vfs,
        vfs_info,
        session_kv,
        http_client,
        mcp_transport,
        llm_provider,
        llm_budget,
        embedding_provider,
        embedding_budget,
        cancel_requested,
        throwaway,
    } = match world {
        Ok(world) => world,
        Err(message) => return fail(ErrorKind::RuntimeError, message),
    };
    let manager = state.session_manager();
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
        audit: execution_audit.clone(),
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
        session_kv,
        llm_provider,
        llm_budget: Some(llm_budget),
        embedding_provider,
        embedding_budget: Some(embedding_budget),
        recording,
        cancel_requested,
        throwaway,
    };
    let packages =
        match state.prepared_packages_for_imports(blueprint_name, &blueprint, &script_imports) {
            Ok(packages) => packages,
            Err(err) => return fail(ErrorKind::PackageResolution, err.to_string()),
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
            telemetry: state.run_telemetry(),
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
    let response = into_response(audit, session_id, &outcome, &console_lines);

    // The success response omits console output; the session keeps the full
    // capture so `/v1/last-run/{id}` can still return it. An MCP request with no
    // session (one rmcp let through) has nowhere to keep it.
    if !session_id.is_empty()
        && let Err(error) = state
            .sessions()
            .record(
                session_id,
                LastRun {
                    result: response.result.clone(),
                    console: console_lines,
                    error: response.error.clone(),
                },
            )
            .await
    {
        // Execution already finished; losing its output would invite a retry of effects.
        tracing::warn!(operation = "record", session = session_id, %error, "last-run storage failed");
    }

    ExecuteOutcome::dispatched(response)
}

/// What a run's world is built from: the run's own inputs, and the cancel signal its
/// caller registered.
pub(crate) struct WorldContext<'a> {
    pub state: &'a AppState,
    pub session_id: &'a str,
    pub blueprint_name: &'a str,
    pub blueprint: &'a Arc<Blueprint>,
    pub variables: &'a Arc<VarBindings>,
    pub harness_secrets: &'a Arc<HarnessSecretBindings>,
    pub network_policy: &'a Arc<interpreter::runtime::NetworkPolicy>,
    pub llm_provider: Option<Arc<dyn interpreter::runtime::LlmProvider>>,
    pub embedding_provider: Option<Arc<dyn interpreter::runtime::EmbeddingProvider>>,
    pub cancel_requested: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// What a run executes in and reaches outside through: its compile-time MCP catalog, its
/// files and data, its connectors, and its token budgets.
pub(crate) struct RunWorld {
    pub mcp_catalog: Arc<crate::record::McpCatalog>,
    pub vfs: Vfs,
    pub vfs_info: VfsInfo,
    pub session_kv: Arc<dyn interpreter::runtime::SessionKvStore>,
    pub http_client: Arc<dyn interpreter::stdlib::http::HttpClient>,
    pub mcp_transport: Arc<dyn interpreter::runtime::McpTransport>,
    pub llm_provider: Option<Arc<dyn interpreter::runtime::LlmProvider>>,
    pub llm_budget: Arc<interpreter::runtime::ExecutionTokenBudget>,
    pub embedding_provider: Option<Arc<dyn interpreter::runtime::EmbeddingProvider>>,
    pub embedding_budget: Arc<interpreter::runtime::EmbeddingTokenBudget>,
    pub cancel_requested: Option<tokio::sync::oneshot::Receiver<()>>,
    /// A test run's throwaway copies, kept until the run's owner task is done with them.
    pub throwaway: Option<Arc<crate::record::Throwaway>>,
}

/// A normal run's world: discovery against its MCP servers, its session's files and data,
/// its session's HTTP client, and budgets charged to the server's.
async fn live_world(
    context: WorldContext<'_>,
    vfs_source: VfsSource,
    mcp_servers: &BTreeSet<String>,
) -> Result<RunWorld, String> {
    let mcp_catalog = context
        .state
        .mcp_catalog_for_imports(
            context.blueprint_name,
            context.blueprint,
            mcp_servers,
            context.harness_secrets,
            context.network_policy,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = ?error, "MCP discovery initialization failed");
            error.to_string()
        })?;
    let manager = context.state.session_manager();
    let (vfs, vfs_info) = match vfs_source {
        VfsSource::Mcp { vfs, vfs_info } => (vfs, vfs_info),
        VfsSource::Rest => {
            open_session(context.state, context.session_id, context.blueprint).await?;
            manager
                .vfs_for_execute_with_variables(
                    context.session_id,
                    context.blueprint,
                    context.variables,
                )
                .await
                .map_err(|err| format!("internal: vfs init failed: {err}"))?
        }
    };
    let (http_client, mcp_transport) = outside_clients(&context);
    Ok(RunWorld {
        mcp_catalog,
        vfs,
        vfs_info,
        session_kv: manager.session_kv_for_execute(context.session_id),
        http_client,
        mcp_transport,
        llm_provider: context.llm_provider,
        llm_budget: manager.llm_budget_for_execute(),
        embedding_provider: context.embedding_provider,
        embedding_budget: manager.embedding_budget_for_execute(),
        cancel_requested: context.cancel_requested,
        throwaway: None,
    })
}

/// Registers the session and marks activity at entry as well as at exit. The idle reaper
/// is wall-clock and has no notion of an execution in flight, so a session that was
/// already nearly idle when this call arrived would otherwise be reaped moments into the
/// run, taking its idempotency reservation with it. This buys the program a full idle
/// window rather than whatever was left of one; it does not make the run un-reapable,
/// and an execution that outlasts `idle_timeout` on its own is still collected
/// mid-flight. The write is debounced (`PERSIST_INTERVAL`), so this costs nothing per call.
pub(crate) async fn open_session(
    state: &AppState,
    session_id: &str,
    blueprint: &Arc<Blueprint>,
) -> Result<(), String> {
    let manager = state.session_manager();
    manager
        .ensure(session_id, blueprint)
        .await
        .map_err(|err| format!("internal: session init failed: {err}"))?;
    manager.touch(session_id).await;
    Ok(())
}

/// The session's HTTP client and the blueprint's MCP transport: what a run reaches
/// outside through when nothing stands in for them.
pub(crate) fn outside_clients(
    context: &WorldContext<'_>,
) -> (
    Arc<dyn interpreter::stdlib::http::HttpClient>,
    Arc<dyn interpreter::runtime::McpTransport>,
) {
    let state = context.state;
    let mcp_transport = submilli_shared::mcp::transport::StreamableHttpTransport::new(
        context.blueprint_name.to_string(),
        Arc::clone(context.blueprint),
        state.oauth_token_manager().cloned(),
        state.secret_store().cloned(),
        Arc::clone(context.network_policy),
    )
    .with_harness_secrets(Arc::clone(context.harness_secrets));
    (
        state.session_manager().http_client(context.session_id),
        Arc::new(mcp_transport),
    )
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
    failure_response(
        crate::audit::execution().as_deref(),
        session_id,
        kind,
        message,
    )
}

/// A failed response, with the failure recorded on the execution's audit.
fn failure_response(
    audit: Option<&crate::audit::ExecutionAudit>,
    session_id: &str,
    kind: ErrorKind,
    message: String,
) -> ExecuteResponse {
    if let Some(audit) = audit {
        audit.error(kind);
    }
    response_for_error(audit, session_id, kind, message)
}

fn response_for_error(
    audit: Option<&crate::audit::ExecutionAudit>,
    session_id: &str,
    kind: ErrorKind,
    message: String,
) -> ExecuteResponse {
    ExecuteResponse {
        execution_id: execution_id_of(audit),
        session_id: session_id.to_string(),
        result: None,
        console: Vec::new(),
        error: Some(ExecuteError {
            kind,
            message,
            diagnostics: Vec::new(),
            denial: None,
        }),
        discovery_warnings: Vec::new(),
    }
}

fn execution_id_of(audit: Option<&crate::audit::ExecutionAudit>) -> String {
    audit.map_or_else(crate::audit::execution_id, |audit| audit.id.clone())
}

fn into_response(
    audit: Option<&crate::audit::ExecutionAudit>,
    session_id: &str,
    outcome: &RunOutcome,
    console_lines: &[String],
) -> ExecuteResponse {
    let (result, console, error) = outcome_to_parts(outcome, console_lines);
    ExecuteResponse {
        execution_id: execution_id_of(audit),
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
fn outcome_to_parts(
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

fn split_console(raw: &str) -> Vec<String> {
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
