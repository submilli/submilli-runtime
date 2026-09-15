//! `submilli server mcp authenticate <blueprint> <server>` — drive the OAuth
//! flow for one MCP server. The CLI runs the browser/PKCE loopback and forwards
//! the authorization code to the server, which holds the client secret and does
//! the token exchange. The CLI never sees a secret.
//!
//! Re-running re-authenticates: the server overwrites the stored credential, so
//! this is how you pick up a new scope set or recover from a revoked token.

use std::net::TcpListener;
use std::process::ExitCode;

use anyhow::{Context, Result};

use super::AuthStateBody;
use super::oauth::{self, AuthConfig};
use crate::commands::http::{client, read_or_error};

/// Default loopback port for the OAuth redirect. Pinned (rather than ephemeral)
/// so it matches a provider's registered callback URL out of the box;
/// `$SUBMILLI_OAUTH_REDIRECT_PORT` overrides it.
const DEFAULT_OAUTH_REDIRECT_PORT: u16 = 8765;

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint that declares the MCP server.
    blueprint: String,
    /// The MCP server's local name (its key in the `mcp:` block).
    server: String,
    /// Base URL of the running submilli-server.
    #[arg(long = "server", default_value = "http://127.0.0.1:8128")]
    server_url: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
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

fn run(args: &Args) -> Result<String> {
    let agent = client();
    let base = args.server_url.trim_end_matches('/');

    // Bind the loopback redirect first — its URL is what the server may need to
    // register a client with (for providers without a configured client id).
    // The port is pinned so it matches a provider's registered callback (e.g. a
    // GitHub App); `$SUBMILLI_OAUTH_REDIRECT_PORT` overrides the default.
    let bind = match std::env::var("SUBMILLI_OAUTH_REDIRECT_PORT") {
        Ok(p) if !p.is_empty() => format!("127.0.0.1:{}", p.trim()),
        _ => format!("127.0.0.1:{DEFAULT_OAUTH_REDIRECT_PORT}"),
    };
    let listener = TcpListener::bind(&bind).context("binding the loopback redirect listener")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let cfg: AuthConfig = read_or_error(
        agent
            .get(&format!(
                "{base}/v1/mcp/{}/{}/auth-config?redirect_uri={}",
                args.blueprint,
                args.server,
                urlencode(&redirect_uri),
            ))
            .call()
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    )?;

    let client_id = cfg
        .client_id
        .context("server returned no client_id (configure mcp_oauth or the blueprint)")?;
    let authorization_endpoint = cfg
        .authorization_endpoint
        .context("server returned no authorization_endpoint (discovery failed)")?;

    let pkce = oauth::pkce()?;
    let state = oauth::random_token(16)?;
    let url = oauth::authorize_url(
        &authorization_endpoint,
        &client_id,
        &cfg.scopes,
        &redirect_uri,
        &pkce.challenge,
        &state,
    );
    println!("Open this URL in your browser to authorize:\n\n  {url}\n");
    println!("Waiting for the redirect on {redirect_uri} …");

    let code = oauth::wait_for_code(&listener, &state)?;

    // Forward the code to the server, which exchanges it with the config-held
    // client secret and stores the credential.
    let st: AuthStateBody = read_or_error(
        agent
            .post(&format!(
                "{base}/v1/mcp/{}/{}/oauth/exchange",
                args.blueprint, args.server
            ))
            .send_json(serde_json::json!({
                "code": code,
                "redirect_uri": redirect_uri,
                "code_verifier": pkce.verifier,
                "client_id": client_id,
            }))
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    )?;

    Ok(format!(
        "authenticated '{}' on blueprint '{}' — {}",
        args.server,
        args.blueprint,
        st.summary(&args.blueprint)
    ))
}

/// Minimal percent-encoding for the redirect_uri query param.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
