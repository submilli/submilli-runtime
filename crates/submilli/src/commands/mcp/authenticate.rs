//! `submilli mcp authenticate --blueprint <path> <server>` — run the full OAuth
//! flow for one MCP server locally and store its credential. This drives
//! discovery, optional DCR, the browser/PKCE loopback, and the token exchange
//! itself. By default it's a **public** client (PKCE, no client secret); when a
//! local provider config ([`super::provider_config`]) matches the OAuth host and
//! supplies a `client_secret`, the exchange runs as a **confidential** client —
//! the flow the server subcommand does, available locally.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use interpreter::runtime::{HttpClient, NetworkPolicy, ReqwestHttpClient};
use submilli_blueprint::{Blueprint, McpAuth, interpolate};
use submilli_shared::BlueprintSecretResolver;
use submilli_shared::mcp::oauth as mcp_oauth;
use submilli_shared::mcp_auth::{OAuthCredential, blueprint_auth_state, write_credential};
use submilli_shared::secret_store::SecretStore;

use crate::commands::local;
use crate::commands::mcp::{load_blueprint, summary};
use crate::commands::server::mcp::oauth::{authorize_url, pkce, random_token, wait_for_code};

/// Default loopback port for the OAuth redirect. Pinned (rather than ephemeral)
/// so it matches a provider's registered callback URL out of the box;
/// `$SUBMILLI_OAUTH_REDIRECT_PORT` overrides it.
const DEFAULT_OAUTH_REDIRECT_PORT: u16 = 8765;

#[derive(clap::Args)]
pub struct Args {
    /// The MCP server's local name (its key in the blueprint's `mcp:` block).
    server: String,
    /// Blueprint file that declares the MCP server.
    #[arg(long)]
    blueprint: PathBuf,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match local::block_on(run(&args))? {
        Ok(message) => {
            println!("✓ {message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

async fn run(args: &Args) -> Result<String> {
    let blueprint = load_blueprint(&args.blueprint)?;
    let server = blueprint
        .mcp
        .get(&args.server)
        .with_context(|| format!("blueprint declares no MCP server '{}'", args.server))?;
    let Some(McpAuth::Oauth2 {
        client_id,
        authorization_endpoint,
        token_endpoint,
        scopes,
    }) = &server.auth
    else {
        bail!(
            "MCP server '{}' does not declare `auth: oauth2`",
            args.server
        );
    };

    let store = local::open_secret_store()?;
    let http =
        Arc::new(ReqwestHttpClient::new(Arc::new(NetworkPolicy::default()))) as Arc<dyn HttpClient>;
    let resolver = BlueprintSecretResolver::new(Some(store.clone()));

    // Bind the loopback redirect first; its URL is what DCR registers as the
    // callback. The port is pinned so a provider's pre-registered callback
    // matches; `$SUBMILLI_OAUTH_REDIRECT_PORT` overrides.
    let listener = bind_redirect_listener()?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    // Resolve the blueprint-pinned fields (each may carry `${secrets.X}` /
    // `${env.X}`); discover anything left unset.
    let mut authz = opt_interpolate(authorization_endpoint, &blueprint, &resolver).await?;
    let mut token = opt_interpolate(token_endpoint, &blueprint, &resolver).await?;
    let mut client_id = opt_interpolate(client_id, &blueprint, &resolver).await?;
    let mut resolved_scopes = Vec::with_capacity(scopes.len());
    for scope in scopes {
        resolved_scopes.push(
            interpolate(scope, &blueprint, &resolver)
                .await
                .map_err(|e| anyhow!("{e}"))?,
        );
    }

    let mut registration_endpoint = None;
    let mut discovered_scopes = Vec::new();
    if authz.is_none() || token.is_none() || client_id.is_none() {
        let endpoints = mcp_oauth::discover(&http, &server.url)
            .await
            .map_err(|e| anyhow!("OAuth discovery failed: {e}"))?;
        if authz.is_none() {
            authz = Some(endpoints.authorization_endpoint);
        }
        if token.is_none() {
            token = Some(endpoints.token_endpoint);
        }
        registration_endpoint = endpoints.registration_endpoint;
        discovered_scopes = endpoints.scopes_supported;
    }
    let authorization_endpoint =
        authz.context("no authorization_endpoint configured and discovery found none")?;
    let token_endpoint = token.context("no token_endpoint configured and discovery found none")?;

    // A configured local provider (matched by the OAuth host) supplies the
    // client_id + client_secret; a resolved client_secret upgrades the exchange
    // from public PKCE to confidential. A reference the operator configured but
    // that doesn't resolve is an error, not a silent downgrade.
    let providers = super::provider_config::load()?;
    let provider = mcp_oauth::match_provider(&providers, &token_endpoint);
    let mut client_secret = None;
    if let Some(provider) = provider {
        if client_id.is_none() {
            client_id =
                Some(resolve_configured_ref(&provider.client_id, &store, "client_id").await?);
        }
        if let Some(raw) = &provider.client_secret {
            client_secret = Some(resolve_configured_ref(raw, &store, "client_secret").await?);
        }
        if resolved_scopes.is_empty() && !provider.scopes.is_empty() {
            resolved_scopes = provider.scopes.clone();
        }
    }

    // client_id: blueprint pin / provider config → Dynamic Client Registration
    // (public client) when the server offers it.
    if client_id.is_none() {
        let reg = registration_endpoint.context(
            "server needs a client_id but advertises no registration endpoint; \
             set `auth.client_id` in the blueprint, or configure a local provider \
             (`submilli mcp provider add`)",
        )?;
        client_id = Some(
            mcp_oauth::register_client(&http, &reg, &redirect_uri)
                .await
                .map_err(|e| anyhow!("dynamic client registration failed: {e}"))?,
        );
    }
    let client_id = client_id.expect("client_id set by pin, provider, or registration above");

    if resolved_scopes.is_empty() {
        resolved_scopes = discovered_scopes;
    }

    let pkce = pkce()?;
    let state = random_token(16)?;
    let url = authorize_url(
        &authorization_endpoint,
        &client_id,
        &resolved_scopes,
        &redirect_uri,
        &pkce.challenge,
        &state,
    );
    println!("Open this URL in your browser to authorize:\n\n  {url}\n");
    println!("Waiting for the redirect on {redirect_uri} …");
    let code = wait_for_code(&listener, &state)?;

    // Confidential when a local provider supplied a client_secret; otherwise a
    // public-client exchange where PKCE alone proves possession.
    let grant = mcp_oauth::exchange_code(
        &http,
        &token_endpoint,
        &client_id,
        client_secret.as_deref(),
        &code,
        &redirect_uri,
        &pkce.verifier,
    )
    .await
    .map_err(|e| anyhow!("token exchange failed: {e}"))?;

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
    write_credential(&blueprint.name, &args.server, &credential, &store)
        .await
        .map_err(|e| anyhow!("storing credential: {e}"))?;

    let state = blueprint_auth_state(&blueprint, Some(&store)).await;
    Ok(format!(
        "authenticated '{}' on blueprint '{}' — {}",
        args.server,
        blueprint.name,
        summary(&blueprint.name, &state)
    ))
}

/// Resolve a provider's `${secrets.X}` / `${env.X}` reference, erroring — rather
/// than silently downgrading a confidential exchange to public — when a
/// reference the operator configured doesn't resolve, naming what to fix.
async fn resolve_configured_ref(
    raw: &str,
    store: &Arc<dyn SecretStore>,
    field: &str,
) -> Result<String> {
    mcp_oauth::resolve_secret_ref(raw, Some(store))
        .await
        .with_context(|| {
            format!(
                "provider {field} `{raw}` did not resolve — for `${{secrets.NAME}}` run \
                 `submilli secret put NAME`; for `${{env.VAR}}` set the variable"
            )
        })
}

/// Resolve an optional blueprint-pinned field's `${secrets.X}` / `${env.X}`
/// placeholders, leaving `None` untouched.
async fn opt_interpolate(
    value: &Option<String>,
    blueprint: &Blueprint,
    resolver: &BlueprintSecretResolver,
) -> Result<Option<String>> {
    match value {
        Some(v) => Ok(Some(
            interpolate(v, blueprint, resolver)
                .await
                .map_err(|e| anyhow!("{e}"))?,
        )),
        None => Ok(None),
    }
}

fn bind_redirect_listener() -> Result<TcpListener> {
    let bind = match std::env::var("SUBMILLI_OAUTH_REDIRECT_PORT") {
        Ok(p) if !p.is_empty() => format!("127.0.0.1:{}", p.trim()),
        _ => format!("127.0.0.1:{DEFAULT_OAUTH_REDIRECT_PORT}"),
    };
    TcpListener::bind(&bind).context("binding the loopback redirect listener")
}
