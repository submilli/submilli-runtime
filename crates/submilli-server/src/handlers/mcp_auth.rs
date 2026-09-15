//! Admin REST surface for MCP OAuth, backing the `submilli server mcp` CLI.
//!
//! The OAuth dance runs on the operator's machine (the CLI); the server's role
//! is to (a) hand the CLI the resolved client config to run the flow with,
//! (b) store the resulting credential, and (c) report each blueprint's
//! PENDING/ACTIVE state. Credentials land in the SecretStore at
//! `mcp_oauth/<blueprint>/<server>/credential`; access tokens never touch
//! this surface.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use submilli_blueprint::{Blueprint, McpAuth, McpServer};
use submilli_shared::EnvFileSecretResolver;

use crate::app::AppState;
use crate::handlers::execute::blueprint_miss_message;
use submilli_shared::mcp_auth::{
    AuthState, OAuthCredential, ServerKind, blueprint_auth_state, credential_key, server_kind,
    write_credential,
};
use submilli_shared::secret_store::SecretStore;

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
    pub message: String,
}

type Failure = (StatusCode, Json<ErrorResponse>);

fn err(status: StatusCode, error: &'static str, message: String) -> Failure {
    (status, Json(ErrorResponse { error, message }))
}

/// The configured store, or a 503 if none is installed (OAuth needs somewhere to
/// keep the refresh token).
fn store(state: &AppState) -> Result<&Arc<dyn SecretStore>, Failure> {
    state.secret_store().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_secret_store",
            "no secret store is configured on this server; OAuth refresh tokens have nowhere to live"
                .into(),
        )
    })
}

async fn get_blueprint(state: &AppState, name: &str) -> Result<Blueprint, Failure> {
    match state.blueprints().get(name).await {
        Some(blueprint) => Ok(blueprint),
        // A name held out of the parsed set is still registered — reporting it as
        // unregistered here would send an operator looking for a blueprint sitting
        // in the store with readable YAML.
        None => Err(err(
            StatusCode::NOT_FOUND,
            "unknown_blueprint",
            blueprint_miss_message(state, name).await,
        )),
    }
}

/// Look up a declared OAuth server, rejecting unknown or non-OAuth ones.
fn oauth_server<'a>(blueprint: &'a Blueprint, server: &str) -> Result<&'a McpServer, Failure> {
    let Some(entry) = blueprint.mcp.get(server) else {
        return Err(err(
            StatusCode::NOT_FOUND,
            "unknown_mcp_server",
            format!(
                "blueprint '{}' declares no mcp server '{server}'",
                blueprint.name
            ),
        ));
    };
    if server_kind(entry) != ServerKind::OAuth {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "not_oauth",
            format!("mcp server '{server}' does not use `auth: oauth2`"),
        ));
    }
    Ok(entry)
}

/// The resolved OAuth client config the CLI needs to run the flow. `${secrets.X}`
/// in the `auth:` block is resolved server-side, since the CLI can't read the
/// (write-only) secret store.
#[derive(Debug, Serialize)]
pub struct AuthConfig {
    /// The MCP endpoint, for `.well-known` discovery.
    pub url: String,
    /// Absent when the server uses Dynamic Client Registration (the CLI registers
    /// a client at authenticate time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

/// Resolve one `${secrets.X}`-bearing auth field, mapping failures to a 400.
async fn resolve_value(
    value: &str,
    blueprint: &Blueprint,
    resolver: &EnvFileSecretResolver,
    server: &str,
) -> Result<String, Failure> {
    submilli_blueprint::interpolate(value, blueprint, resolver)
        .await
        .map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                "secret_resolve",
                format!("resolving mcp '{server}' auth config: {e}"),
            )
        })
}

/// Query for [`auth_config`]: the caller's `redirect_uri`, needed only when the
/// server must Dynamic-Client-Register (no configured provider) — the registered
/// client is bound to that redirect.
#[derive(Debug, Deserialize)]
pub struct AuthConfigQuery {
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

/// `GET /v1/mcp/{blueprint}/{server}/auth-config` — everything a client needs to
/// build the authorize URL: endpoints (discovered server-side if not pinned) and
/// a `client_id` (blueprint pin → configured provider → Dynamic Client
/// Registration). Never returns a secret.
pub async fn auth_config(
    State(state): State<AppState>,
    Path((blueprint, server)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<AuthConfigQuery>,
) -> Result<Json<AuthConfig>, Failure> {
    let bp = get_blueprint(&state, &blueprint).await?;
    let entry = oauth_server(&bp, &server)?;
    let McpAuth::Oauth2 {
        client_id,
        authorization_endpoint,
        token_endpoint,
        scopes,
    } = entry
        .auth
        .as_ref()
        .expect("oauth_server checked the variant");

    let resolver = EnvFileSecretResolver::new(state.secret_store().cloned());
    let mut client_id = match client_id {
        Some(v) => Some(resolve_value(v, &bp, &resolver, &server).await?),
        None => None,
    };
    let mut authorization_endpoint = match authorization_endpoint {
        Some(v) => Some(resolve_value(v, &bp, &resolver, &server).await?),
        None => None,
    };
    let mut token_endpoint = match token_endpoint {
        Some(v) => Some(resolve_value(v, &bp, &resolver, &server).await?),
        None => None,
    };
    let mut resolved_scopes = Vec::with_capacity(scopes.len());
    for scope in scopes {
        resolved_scopes.push(resolve_value(scope, &bp, &resolver, &server).await?);
    }

    // Discover anything the blueprint didn't pin (and the registration endpoint).
    let mut registration_endpoint = None;
    let mut discovered_scopes = Vec::new();
    if authorization_endpoint.is_none() || token_endpoint.is_none() {
        let endpoints = submilli_shared::mcp::oauth::discover(state.oauth_http(), &entry.url)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, "discovery_failed", e))?;
        authorization_endpoint.get_or_insert(endpoints.authorization_endpoint);
        token_endpoint.get_or_insert(endpoints.token_endpoint);
        registration_endpoint = endpoints.registration_endpoint;
        discovered_scopes = endpoints.scopes_supported;
    }

    // client_id: blueprint pin → configured provider (matched by auth host) → DCR.
    // A provider's client_id may itself be a `${env.X}`/`${secrets.X}` reference.
    let provider_match = authorization_endpoint.as_deref().and_then(|authz| {
        submilli_shared::mcp::oauth::match_provider(state.mcp_oauth_providers(), authz)
            .map(|p| (p.client_id.clone(), p.scopes.clone()))
    });
    if client_id.is_none()
        && let Some((raw_client_id, provider_scopes)) = provider_match
    {
        client_id =
            submilli_shared::mcp::oauth::resolve_secret_ref(&raw_client_id, state.secret_store())
                .await;
        if resolved_scopes.is_empty() {
            resolved_scopes = provider_scopes;
        }
    }
    if client_id.is_none()
        && let (Some(reg), Some(redirect_uri)) = (&registration_endpoint, &query.redirect_uri)
    {
        client_id = Some(
            submilli_shared::mcp::oauth::register_client(state.oauth_http(), reg, redirect_uri)
                .await
                .map_err(|e| err(StatusCode::BAD_GATEWAY, "registration_failed", e))?,
        );
    }

    // Scope precedence: blueprint pin → configured provider → the resource's
    // advertised `scopes_supported`. The last means auth works out of the box
    // for a server that publishes its scopes (e.g. GitHub).
    if resolved_scopes.is_empty() {
        resolved_scopes = discovered_scopes;
    }

    Ok(Json(AuthConfig {
        url: entry.url.clone(),
        client_id,
        authorization_endpoint,
        token_endpoint,
        scopes: resolved_scopes,
    }))
}

/// Body for [`oauth_exchange`] — the code captured by the client, plus the PKCE
/// verifier and the redirect_uri it used. No secret: the server supplies it.
#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
    pub redirect_uri: String,
    pub code_verifier: String,
    /// The client_id used to build the authorize URL (from `auth-config`).
    #[serde(default)]
    pub client_id: Option<String>,
}

/// `POST /v1/mcp/{blueprint}/{server}/oauth/exchange` — exchange the authorization
/// code for a token using the server-held `client_secret` (config, matched by the
/// token-endpoint host), then store the credential. The secret never leaves here.
pub async fn oauth_exchange(
    State(state): State<AppState>,
    Path((blueprint, server)): Path<(String, String)>,
    Json(req): Json<ExchangeRequest>,
) -> Result<Json<AuthStateResponse>, Failure> {
    let bp = get_blueprint(&state, &blueprint).await?;
    let entry = oauth_server(&bp, &server)?;
    let store = store(&state)?;
    let http = state.oauth_http();

    // Resolve the token endpoint: blueprint pin, else discover.
    let resolver = EnvFileSecretResolver::new(state.secret_store().cloned());
    let McpAuth::Oauth2 { token_endpoint, .. } = entry.auth.as_ref().expect("checked");
    let token_endpoint = match token_endpoint {
        Some(v) => resolve_value(v, &bp, &resolver, &server).await?,
        None => {
            submilli_shared::mcp::oauth::discover(http, &entry.url)
                .await
                .map_err(|e| err(StatusCode::BAD_GATEWAY, "discovery_failed", e))?
                .token_endpoint
        }
    };

    // The configured provider (matched by token host) supplies client_id + secret
    // (each may be a `${env.X}`/`${secrets.X}` reference); otherwise fall back to the
    // client-supplied (e.g. DCR'd) client_id, no secret.
    let provider =
        submilli_shared::mcp::oauth::match_provider(state.mcp_oauth_providers(), &token_endpoint)
            .map(|p| (p.client_id.clone(), p.client_secret.clone()));
    let (client_id, client_secret) = match provider {
        Some((raw_id, raw_secret)) => {
            let id = submilli_shared::mcp::oauth::resolve_secret_ref(&raw_id, state.secret_store())
                .await;
            let secret = match raw_secret {
                Some(raw) => {
                    submilli_shared::mcp::oauth::resolve_secret_ref(&raw, state.secret_store())
                        .await
                }
                None => None,
            };
            (id, secret)
        }
        None => (req.client_id, None),
    };
    let client_id = client_id.ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "missing_client_id",
            "no client_id from config or request".to_string(),
        )
    })?;

    let grant = submilli_shared::mcp::oauth::exchange_code(
        http,
        &token_endpoint,
        &client_id,
        client_secret.as_deref(),
        &req.code,
        &req.redirect_uri,
        &req.code_verifier,
    )
    .await
    .map_err(|e| err(StatusCode::BAD_GATEWAY, "exchange_failed", e))?;

    // Refresh token preferred; else store the static access token (no refresh).
    let credential = if grant.refresh_token.is_some() {
        OAuthCredential {
            refresh_token: grant.refresh_token,
            access_token: None,
            client_id: Some(client_id),
            token_endpoint: Some(token_endpoint),
            scopes: Vec::new(),
        }
    } else {
        OAuthCredential {
            refresh_token: None,
            access_token: grant.access_token,
            client_id: Some(client_id),
            token_endpoint: None,
            scopes: Vec::new(),
        }
    };
    write_credential(&blueprint, &server, &credential, store)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                e.to_string(),
            )
        })?;
    Ok(Json(blueprint_auth_state(&bp, Some(store)).await.into()))
}

/// Mirror of [`AuthState`] for the wire.
#[derive(Debug, Serialize)]
pub struct AuthStateResponse {
    /// `"active"` or `"pending"`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unauthenticated: Vec<String>,
}

impl From<AuthState> for AuthStateResponse {
    fn from(state: AuthState) -> Self {
        match state {
            AuthState::Active => AuthStateResponse {
                state: "active",
                unauthenticated: Vec::new(),
            },
            AuthState::Pending { unauthenticated } => AuthStateResponse {
                state: "pending",
                unauthenticated,
            },
        }
    }
}

/// The credential the CLI deposits after running the OAuth flow: the refresh
/// token plus the inputs the runtime redeems it with at call time (the exact
/// `client_id` used — incl. a Dynamic-Client-Registered one — the resolved
/// `token_endpoint`, and scopes).
#[derive(Debug, Deserialize)]
pub struct CredentialRequest {
    /// Standard OAuth2 refresh token. Mutually exclusive with `access_token`.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Static, non-expiring access token (e.g. GitHub OAuth App). Mutually
    /// exclusive with `refresh_token`.
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    /// Required with `refresh_token`; unused for a static `access_token`.
    #[serde(default)]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// `POST /v1/mcp/{blueprint}/{server}/refresh-token` — deposit the credential the
/// CLI obtained, then report the blueprint's new state.
pub async fn put_refresh_token(
    State(state): State<AppState>,
    Path((blueprint, server)): Path<(String, String)>,
    Json(req): Json<CredentialRequest>,
) -> Result<Json<AuthStateResponse>, Failure> {
    let bp = get_blueprint(&state, &blueprint).await?;
    oauth_server(&bp, &server)?;
    let store = store(&state)?;
    // Exactly one token mode: a refresh token (needs a token endpoint to redeem
    // it) or a static access token.
    let credential = match (req.refresh_token, req.access_token) {
        (Some(refresh_token), None) => {
            let token_endpoint = req.token_endpoint.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "missing_token_endpoint",
                    "refresh_token requires token_endpoint".to_string(),
                )
            })?;
            OAuthCredential {
                refresh_token: Some(refresh_token),
                access_token: None,
                client_id: req.client_id,
                token_endpoint: Some(token_endpoint),
                scopes: req.scopes,
            }
        }
        (None, Some(access_token)) => OAuthCredential {
            refresh_token: None,
            access_token: Some(access_token),
            client_id: req.client_id,
            token_endpoint: None,
            scopes: req.scopes,
        },
        _ => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_credential",
                "provide exactly one of refresh_token or access_token".to_string(),
            ));
        }
    };
    write_credential(&blueprint, &server, &credential, store)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                e.to_string(),
            )
        })?;
    Ok(Json(blueprint_auth_state(&bp, Some(store)).await.into()))
}

/// `DELETE /v1/mcp/{blueprint}/{server}/refresh-token` — deauthenticate.
pub async fn delete_refresh_token(
    State(state): State<AppState>,
    Path((blueprint, server)): Path<(String, String)>,
) -> Result<Json<AuthStateResponse>, Failure> {
    let bp = get_blueprint(&state, &blueprint).await?;
    oauth_server(&bp, &server)?;
    let store = store(&state)?;
    store
        .delete(&credential_key(&blueprint, &server))
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                e.to_string(),
            )
        })?;
    Ok(Json(blueprint_auth_state(&bp, Some(store)).await.into()))
}

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub name: String,
    /// `"oauth"`, `"static"`, or `"none"`.
    pub kind: &'static str,
    /// `Some` only for OAuth servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticated: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub blueprint: String,
    pub state: &'static str,
    pub servers: Vec<ServerStatus>,
}

/// `GET /v1/mcp/{blueprint}/auth-status` — per-server rows + overall state.
pub async fn auth_status(
    State(state): State<AppState>,
    Path(blueprint): Path<String>,
) -> Result<Json<AuthStatusResponse>, Failure> {
    let bp = get_blueprint(&state, &blueprint).await?;
    let derived = blueprint_auth_state(&bp, state.secret_store()).await;
    let unauthenticated: Vec<&String> = match &derived {
        AuthState::Pending { unauthenticated } => unauthenticated.iter().collect(),
        AuthState::Active => Vec::new(),
    };

    let servers = bp
        .mcp
        .iter()
        .map(|(name, entry)| {
            let (kind, authenticated) = match server_kind(entry) {
                ServerKind::OAuth => ("oauth", Some(!unauthenticated.contains(&name))),
                ServerKind::StaticKey => ("static", None),
                ServerKind::None => ("none", None),
            };
            ServerStatus {
                name: name.clone(),
                kind,
                authenticated,
            }
        })
        .collect();

    Ok(Json(AuthStatusResponse {
        state: if derived.is_pending() {
            "pending"
        } else {
            "active"
        },
        blueprint,
        servers,
    }))
}
