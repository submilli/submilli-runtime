//! Per-blueprint `StreamableHttpService` cache + the `/mcp/{blueprint}` axum
//! handler that delegates to it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use rmcp::transport::common::http_header::HEADER_SESSION_ID;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use submilli_blueprint::Blueprint;
use tower::ServiceExt;

use crate::app::AppState;
use crate::handlers::execute::blueprint_miss_message;
use crate::mcp::server::SubmilliMcp;
use crate::mcp::session::{RmcpSessionStore, VfsSessionManager, initialize_binding_error};

pub(crate) struct McpService {
    transport: StreamableHttpService<SubmilliMcp, VfsSessionManager>,
    sessions: Arc<VfsSessionManager>,
    deletion: tokio::sync::Mutex<()>,
}

/// Lazily-built MCP service per blueprint name. A service outlives blueprint
/// updates so its live `MCP-Session-Id` map survives — the execute path and
/// `list_tools` re-fetch the blueprint per call. Only blueprint *removal* drops
/// the service (see `AppState::evict_mcp_service`).
pub(crate) type BlueprintServiceCache = Mutex<HashMap<String, Arc<McpService>>>;

pub(crate) fn new_service_cache() -> BlueprintServiceCache {
    Mutex::new(HashMap::new())
}

/// All methods (POST/GET/DELETE) on the MCP endpoint route here; rmcp's service
/// dispatches by method internally.
pub(crate) async fn mcp_handler(
    State(state): State<AppState>,
    Path(blueprint): Path<String>,
    req: Request,
) -> Response {
    let found = match state.blueprints().get(&blueprint).await {
        Ok(found) => found,
        Err(error) => return crate::blueprint::store_failure_response(error).into_response(),
    };
    let Some(bp) = found else {
        // The blueprint is part of the endpoint identity; a name with no runnable
        // blueprint is an addressing failure, distinct from an MCP handshake
        // rejection.
        let message = match blueprint_miss_message(&state, &blueprint).await {
            Ok(message) => message,
            Err(error) => return crate::blueprint::store_failure_response(error).into_response(),
        };
        return (StatusCode::NOT_FOUND, message).into_response();
    };

    // Validate `initialize` variables here, not in rmcp's session layer: rmcp maps
    // any `initialize_session` error to an unlogged HTTP 500, whereas here we can
    // return a logged 400 — and validating against the just-fetched `bp` reflects
    // the latest `apply` even though the cached service predates it.
    let req = match reject_invalid_variables(&blueprint, &bp, req).await {
        Ok(req) => req,
        Err(response) => return *response,
    };

    // An unauthenticated/unavailable MCP server no longer blocks the blueprint: it
    // is simply omitted from discovery (with a warning) so the rest of the
    // blueprint stays usable. See `discover_all`.
    let service = get_or_build(&state, &blueprint, &bp);
    let terminates_session = req.method() == Method::DELETE;
    // Serialize DELETE requests so concurrent retries observe the first
    // deletion before checking whether the session still exists.
    let _deletion = if terminates_session {
        let guard = service.deletion.lock().await;
        if let Some(id) = req
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|v| v.to_str().ok())
        {
            match service.sessions.contains_session(id).await {
                Ok(true) => {}
                Ok(false) => return StatusCode::NOT_FOUND.into_response(),
                Err(error) => {
                    return crate::blueprint::store_failure_response(error).into_response();
                }
            }
        }
        Some(guard)
    } else {
        None
    };

    // `oneshot` consumes the service; the inner state is `Arc`-shared, so the
    // clone is cheap and shares sessions across requests.
    let mut response = match service.transport.clone().oneshot(req).await {
        Ok(resp) => resp.map(Body::new),
        Err(infallible) => match infallible {},
    };
    // rmcp answers a termination with 202, but the session is already closed
    // and wiped by then, and MCP clients that accept only 200 or 204 report the
    // 202 as a failed termination.
    if terminates_session && response.status() == StatusCode::ACCEPTED {
        *response.status_mut() = StatusCode::NO_CONTENT;
    }
    response
}

/// Reject an `initialize` whose `${vars.NAME}` bindings don't satisfy the
/// blueprint, with a logged 400. Buffering the body (as rmcp does, unbounded)
/// lets us inspect it and hand the exact bytes onward; a non-`initialize` or
/// unparseable body passes straight through for rmcp to handle.
async fn reject_invalid_variables(
    name: &str,
    blueprint: &Blueprint,
    req: Request,
) -> Result<Request, Box<Response>> {
    if req.method() != Method::POST {
        return Ok(req);
    }
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        // An unreadable body is rmcp's to reject (it returns its own error); we
        // can't reconstruct it, so surface a 400 rather than a silent hang.
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "unreadable request body").into_response(),
        ));
    };
    if let Some(err) = initialize_binding_error(blueprint, &parts, &bytes) {
        tracing::warn!(blueprint = name, error = %err, "rejecting MCP initialize: invalid bindings");
        return Err(Box::new((StatusCode::BAD_REQUEST, err).into_response()));
    }
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

fn get_or_build(state: &AppState, name: &str, blueprint: &Blueprint) -> Arc<McpService> {
    // Poison means a panic may have interrupted service registration or eviction.
    // AGENTS.md permits poisoned-lock panics rather than reusing partial cache state;
    // it does not permit the panic that caused poisoning. See AppStateInner's field.
    let mut cache = state
        .mcp_services()
        .lock()
        .expect("mcp service cache poisoned");
    if let Some(service) = cache.get(name) {
        return service.clone();
    }

    // Every blueprint runs stateful so each connection has an `MCP-Session-Id`
    // — the key `lastRun` stores its run under. `json_response` only applied in
    // stateless mode, so it's irrelevant here.
    let mut config = StreamableHttpServerConfig::default()
        .with_stateful_mode(true)
        .with_sse_keep_alive(None)
        .with_sse_retry(None);
    if let Some(hosts) = state.mcp_allowed_hosts() {
        config = config.with_allowed_hosts(hosts.to_vec());
    }

    let blueprint_owned = blueprint.clone();
    let session_manager = Arc::new(VfsSessionManager::new(state.clone(), name.to_string()));
    config.session_store = Some(Arc::new(RmcpSessionStore::new(
        state.session_store().clone(),
        state.session_manager().clone(),
        blueprint_owned.clone(),
    )));
    let state_for_factory = state.clone();
    let name_owned = name.to_string();
    let factory = move || {
        Ok(SubmilliMcp::new(
            state_for_factory.clone(),
            name_owned.clone(),
            blueprint_owned.clone(),
        ))
    };

    let service = Arc::new(McpService {
        transport: StreamableHttpService::new(factory, session_manager.clone(), config),
        sessions: session_manager,
        deletion: tokio::sync::Mutex::new(()),
    });
    cache.insert(name.to_string(), service.clone());
    service
}
