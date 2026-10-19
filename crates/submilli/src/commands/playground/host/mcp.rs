//! The host's MCP endpoint, `/mcp/{blueprint}`: the eight tools an agent on the
//! production server sees, served over rmcp's streamable-HTTP transport.
//!
//! Copied from `submilli-server`'s `mcp/router.rs` (the handler, the
//! `initialize` pre-check, DELETE answered 204 once the session is closed). The
//! host serves one blueprint, so it builds one service, at start. Every request
//! needs one of the playground's tokens, and rmcp's default `Host` check stays
//! on, so only a loopback name reaches the endpoint.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use rmcp::transport::common::http_header::HEADER_SESSION_ID;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use submilli_blueprint::{Blueprint, HarnessSecretBindings};
use submilli_build::PackageStore;
use tower::ServiceExt;

use super::auth::{ApiToken, authenticate};
use super::blueprints::{AppliedBlueprint, Blueprints};
use super::run::RunHost;
use super::sessions::Sessions;
use super::volumes::Volumes;

mod packages;
mod session;
mod tools;

#[cfg(test)]
mod tests;

use session::{EXPLICIT_DELETE, HostSessionManager, HostSessionStore, initialize_binding_error};
use tools::SubmilliMcp;

/// What the endpoint's sessions and tools share.
pub(crate) struct McpParts {
    pub(crate) run: Arc<RunHost>,
    pub(crate) sessions: Arc<Sessions>,
    pub(crate) blueprints: Arc<Blueprints>,
    pub(crate) volumes: Arc<Volumes>,
    pub(crate) package_store: PackageStore,
    pub(crate) tokens: Arc<[ApiToken]>,
}

struct Shared {
    run: Arc<RunHost>,
    sessions: Arc<Sessions>,
    blueprints: Arc<Blueprints>,
    volumes: Arc<Volumes>,
    package_store: PackageStore,
}

impl Shared {
    /// The blueprint in force, when it is the one `name` addresses.
    async fn blueprint(&self, name: &str) -> Option<AppliedBlueprint> {
        self.blueprints
            .for_run()
            .await
            .filter(|applied| applied.blueprint.name == name)
    }

    fn secrets(&self, id: &str) -> Option<Arc<HarnessSecretBindings>> {
        self.sessions.secrets(id)
    }

    fn bind_secrets(&self, id: &str, secrets: HarnessSecretBindings) {
        self.sessions.bind_secrets(id, Arc::new(secrets));
    }

    fn forget_secrets(&self, id: &str) {
        self.sessions.forget_secrets(id);
    }
}

/// The endpoint, ready to mount with [`routes`].
pub(crate) struct McpHost {
    transport: StreamableHttpService<SubmilliMcp, HostSessionManager>,
    sessions: Arc<HostSessionManager>,
    shared: Arc<Shared>,
    tokens: Arc<[ApiToken]>,
    /// Serializes DELETEs, so a retry sees the first one's close.
    deletion: tokio::sync::Mutex<()>,
}

/// Who called, by the name of the token they presented.
#[derive(Clone)]
struct Caller(String);

impl McpHost {
    /// The endpoint for the blueprint the host serves, `blueprint_name`.
    pub(crate) fn new(parts: McpParts, blueprint_name: &str) -> Self {
        let shared = Arc::new(Shared {
            run: parts.run,
            sessions: parts.sessions,
            blueprints: parts.blueprints,
            volumes: parts.volumes,
            package_store: parts.package_store,
        });
        // Every session is stateful, so each connection has an `MCP-Session-Id`,
        // the id its runs, files and values are kept under. The default allowed
        // hosts stay: `localhost`, `127.0.0.1` and `::1`.
        let mut config = StreamableHttpServerConfig::default()
            .with_stateful_mode(true)
            .with_sse_keep_alive(None)
            .with_sse_retry(None);
        config.session_store = Some(Arc::new(HostSessionStore::new(
            Arc::clone(&shared),
            blueprint_name.to_owned(),
        )));
        let sessions = Arc::new(HostSessionManager::new(
            Arc::clone(&shared),
            blueprint_name.to_owned(),
        ));
        let factory = {
            let shared = Arc::clone(&shared);
            let name = blueprint_name.to_owned();
            move || Ok(SubmilliMcp::new(Arc::clone(&shared), name.clone()))
        };
        Self {
            transport: StreamableHttpService::new(factory, Arc::clone(&sessions), config),
            sessions,
            shared,
            tokens: parts.tokens,
            deletion: tokio::sync::Mutex::new(()),
        }
    }
}

/// `/mcp/{blueprint}`, every method.
pub(crate) fn routes(host: Arc<McpHost>) -> Router {
    Router::new()
        .route("/mcp/{blueprint}", any(handle))
        .with_state(host)
}

async fn handle(
    State(host): State<Arc<McpHost>>,
    Path(blueprint): Path<String>,
    mut req: Request,
) -> Response {
    let Some(token) = authenticate(&host.tokens, req.headers()) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    req.extensions_mut().insert(Caller(token.name().to_owned()));
    let Some(applied) = host.shared.blueprint(&blueprint).await else {
        return (
            StatusCode::NOT_FOUND,
            format!("unknown blueprint: {blueprint}"),
        )
            .into_response();
    };

    // Checked here, not in rmcp's session layer, which answers any
    // `initialize_session` error with a bare 500.
    let req = match reject_invalid_variables(&applied.blueprint, req).await {
        Ok(req) => req,
        Err(response) => return *response,
    };

    let terminates_session = req.method() == Method::DELETE;
    let _deletion = if terminates_session {
        let guard = host.deletion.lock().await;
        if let Some(id) = req
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|v| v.to_str().ok())
        {
            match host.sessions.contains_session(id).await {
                Ok(true) => {}
                Ok(false) => return StatusCode::NOT_FOUND.into_response(),
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("session storage unavailable: {error:#}"),
                    )
                        .into_response();
                }
            }
        }
        Some(guard)
    } else {
        None
    };

    let mut response = match EXPLICIT_DELETE
        .scope(terminates_session, host.transport.clone().oneshot(req))
        .await
    {
        Ok(resp) => resp.map(Body::new),
        Err(infallible) => match infallible {},
    };
    // rmcp answers a termination with 202, but the session is already closed by
    // then, and MCP clients that accept only 200 or 204 report the 202 as a
    // failed termination.
    if terminates_session && response.status() == StatusCode::ACCEPTED {
        *response.status_mut() = StatusCode::NO_CONTENT;
    }
    response
}

/// Refuse, with a 400, an `initialize` whose variables or secrets do not satisfy
/// `blueprint`. Anything else passes through for rmcp.
async fn reject_invalid_variables(
    blueprint: &Blueprint,
    req: Request,
) -> Result<Request, Box<Response>> {
    if req.method() != Method::POST {
        return Ok(req);
    }
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return Err(Box::new(
            (StatusCode::BAD_REQUEST, "unreadable request body").into_response(),
        ));
    };
    if let Some(err) = initialize_binding_error(blueprint, &parts, &bytes) {
        return Err(Box::new((StatusCode::BAD_REQUEST, err).into_response()));
    }
    Ok(Request::from_parts(parts, Body::from(bytes)))
}
