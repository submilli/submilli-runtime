//! Inbound authentication as a caller sees it: which routes need which token,
//! and what a refusal looks like.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use submilli_server::{
    Access, ApiToken, AppState, AuthConfig, Role, ServerConfig, app, route_table,
};
use tower::ServiceExt;

const ADMIN: &str = "admin-token-0123456789abcdef0123456789";
const ADMIN_NEXT: &str = "admin-next-0123456789abcdef01234567890";
const USER: &str = "user-token-0123456789abcdef01234567890";

/// A server requiring tokens, with two admin entries the way a rotation in
/// progress has them. The package store is a directory of its own so an admin
/// request that reaches a handler never reads the developer's.
fn router() -> (Router, tempfile::TempDir) {
    let packages = tempfile::tempdir().expect("temp package store");
    let token = |name, role, token| ApiToken::new(name, role, token).expect("valid token");
    let router = app(futures::executor::block_on(AppState::new(ServerConfig {
        auth: AuthConfig::Tokens(vec![
            token("ops", Role::Admin, ADMIN),
            token("ops-next", Role::Admin, ADMIN_NEXT),
            token("app", Role::User, USER),
        ]),
        package_store_root: Some(packages.path().to_path_buf()),
        ..in_memory_config::config()
    }))
    .expect("build AppState"));
    (router, packages)
}

async fn send(
    router: &Router,
    method: Method,
    path: &str,
    authorization: Option<&str>,
) -> Response<Body> {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(value) = authorization {
        request = request.header(AUTHORIZATION, value);
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response")
}

async fn status(router: &Router, method: Method, path: &str, token: Option<&str>) -> StatusCode {
    let bearer = token.map(|token| format!("Bearer {token}"));
    send(router, method, path, bearer.as_deref()).await.status()
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json body")
}

/// A concrete path for a route pattern: every `{param}` and `{*wildcard}`
/// segment becomes a literal.
fn concrete(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|segment| {
            if segment.starts_with('{') {
                "x"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The classification is the security boundary, so it is pinned here in full:
/// moving a route between levels, or adding one, has to change this list.
#[test]
fn every_route_declares_the_access_it_requires() {
    use Access::{Admin, Public, User};
    let expected = [
        ("/healthz", Public),
        ("/v1/status", Admin),
        ("/v1/shutdown", Admin),
        ("/v1/execute", User),
        ("/v1/sessions", User),
        ("/v1/sessions/{session_id}/execute", User),
        ("/v1/sessions/{session_id}/rebind", User),
        ("/v1/sessions/{session_id}/last-run", User),
        ("/v1/sessions/{session_id}", User),
        ("/v1/blueprints", Admin),
        ("/v1/blueprints/{name}", Admin),
        ("/v1/blueprints/{name}/prompt", User),
        ("/v1/secrets", Admin),
        ("/v1/secrets/{*key}", Admin),
        ("/v1/packages", Admin),
        ("/v1/packages/{*name}", Admin),
        ("/v1/packages/install", Admin),
        ("/v1/blueprints/{name}/packages/search", User),
        ("/v1/blueprints/{name}/packages/docs", User),
        ("/v1/blueprints/{name}/builtins", User),
        ("/v1/blueprints/{name}/builtins/docs", User),
        ("/v1/capabilities", Admin),
        ("/v1/volumes", Admin),
        ("/v1/mcp/{blueprint}/auth-status", Admin),
        ("/v1/mcp/{blueprint}/{server}/auth-config", Admin),
        ("/v1/mcp/{blueprint}/{server}/refresh-token", Admin),
        ("/v1/mcp/{blueprint}/{server}/oauth/exchange", Admin),
        ("/mcp/{blueprint}", User),
    ];
    assert_eq!(route_table(), expected);
}

/// Walks the server's own route table, so a route added later is covered
/// without being listed here. `PATCH` is a method no route serves: a request
/// that clears the guard answers 405 without running a handler, and one that
/// does not is refused before the router says which methods exist.
#[tokio::test]
async fn each_route_enforces_its_access_level() {
    let (router, _packages) = router();
    for (pattern, access) in route_table() {
        let path = concrete(pattern);
        let anonymous = status(&router, Method::PATCH, &path, None).await;
        let user = status(&router, Method::PATCH, &path, Some(USER)).await;
        let admin = status(&router, Method::PATCH, &path, Some(ADMIN)).await;
        let allowed = StatusCode::METHOD_NOT_ALLOWED;
        let expected = match access {
            Access::Public => (allowed, allowed, allowed),
            Access::User => (StatusCode::UNAUTHORIZED, allowed, allowed),
            Access::Admin => (StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN, allowed),
        };
        assert_eq!((anonymous, user, admin), expected, "{pattern}");
    }
}

#[tokio::test]
async fn a_valid_token_reaches_the_handler() {
    let (router, _packages) = router();
    assert_eq!(
        status(&router, Method::GET, "/healthz", None).await,
        StatusCode::OK
    );
    for token in [ADMIN, ADMIN_NEXT] {
        let reached = status(&router, Method::GET, "/v1/status", Some(token)).await;
        assert_eq!(reached, StatusCode::OK);
    }
    // The blueprint does not exist, which only the handler can know.
    let reached = status(
        &router,
        Method::GET,
        "/v1/blueprints/nope/prompt",
        Some(USER),
    )
    .await;
    assert_eq!(reached, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn anything_but_one_known_bearer_token_is_unauthorized() {
    let (router, _packages) = router();
    let wrong = "wrong-token-0123456789abcdef0123456789";
    for authorization in [
        None,
        Some(format!("Bearer {wrong}")),
        Some(format!("Basic {ADMIN}")),
        Some(ADMIN.to_string()),
        Some("Bearer ".to_string()),
        Some(format!("Bearer {ADMIN} ")),
    ] {
        let response = send(&router, Method::GET, "/v1/status", authorization.as_deref()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{authorization:?}"
        );
    }
}

/// An MCP client that follows the authorization spec treats `resource_metadata`
/// in the challenge, or a discovery document, as an invitation to start OAuth.
/// There is no authorization server, so the refusal must offer neither.
#[tokio::test]
async fn a_refusal_does_not_invite_oauth_discovery() {
    let (router, _packages) = router();
    let response = send(&router, Method::POST, "/mcp/support", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let challenge = response.headers().get(WWW_AUTHENTICATE).expect("challenge");
    assert_eq!(challenge, "Bearer");
    let body = json_body(response).await;
    assert_eq!(body["error"], "unauthorized");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("Authorization: Bearer"))
    );

    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp/support",
        "/.well-known/oauth-authorization-server",
        "/no/such/route",
    ] {
        for token in [None, Some(ADMIN)] {
            let found = status(&router, Method::GET, path, token).await;
            assert_eq!(found, StatusCode::NOT_FOUND, "{path}");
        }
    }
}

#[tokio::test]
async fn a_forbidden_response_names_the_role_needed() {
    let (router, _packages) = router();
    let response = send(
        &router,
        Method::GET,
        "/v1/status",
        Some(&format!("Bearer {USER}")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
    let body = json_body(response).await;
    assert_eq!(body["error"], "forbidden");
    let message = body["message"].as_str().expect("message");
    assert!(
        message.contains("`admin`") && message.contains("`user`"),
        "{message}"
    );
}

#[tokio::test]
async fn a_server_without_tokens_admits_every_caller() {
    let router = app(AppState::new(in_memory_config::config())
        .await
        .expect("build AppState"));
    assert_eq!(
        status(&router, Method::GET, "/v1/status", None).await,
        StatusCode::OK
    );
    // A token nobody configured is ignored rather than refused.
    assert_eq!(
        status(&router, Method::GET, "/v1/status", Some(USER)).await,
        StatusCode::OK
    );
}
