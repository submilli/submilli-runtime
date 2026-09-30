//! Shared HTTP plumbing for commands that talk to a server: which server and
//! with what token, a ureq client that surfaces non-2xx bodies instead of
//! erroring, and the `{ error, message }` envelope both servers use for
//! rejections.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::http::header::AUTHORIZATION;
use ureq::http::{HeaderValue, StatusCode};

/// Holds the token when no `--token-file` is given. Named for the admin role
/// because nearly every `submilli server` command manages the server; the two
/// that only run or read (`run-code`, `docs`) accept a user token in it too.
const ADMIN_TOKEN_ENV: &str = "SUBMILLI_ADMIN_TOKEN";
const TOKEN_FILE_ENV: &str = "SUBMILLI_SERVER_TOKEN_FILE";
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8128";

/// The submilli-server a command talks to, and the token it presents.
#[derive(clap::Args)]
pub struct ServerTarget {
    /// Base URL of the running submilli-server.
    #[arg(
        long = "server",
        env = "SUBMILLI_SERVER_URL",
        value_name = "URL",
        default_value = DEFAULT_SERVER_URL
    )]
    server_url: String,

    /// File holding the API token to send. Without it the token is read from
    /// `$SUBMILLI_ADMIN_TOKEN`; with neither, no token is sent, which only a
    /// server started with `--allow-unauthenticated` accepts. There is no flag
    /// taking the token itself, so it never lands in the process list.
    /// Env: `$SUBMILLI_SERVER_TOKEN_FILE`.
    #[arg(long, value_name = "PATH")]
    token_file: Option<PathBuf>,
}

impl ServerTarget {
    /// The base URL without a trailing slash, ready to have a path appended.
    ///
    /// An exported-but-empty `$SUBMILLI_SERVER_URL` means the default, not a
    /// server at no address: `server stop` reads an unreachable server as one
    /// that is already down.
    pub fn base(&self) -> &str {
        match self.server_url.trim().trim_end_matches('/') {
            "" => DEFAULT_SERVER_URL,
            base => base,
        }
    }

    pub fn agent(&self) -> Result<ureq::Agent> {
        self.agent_with_timeout(None)
    }

    /// `timeout` bounds the whole call; `None` waits as long as the server does.
    pub fn agent_with_timeout(&self, timeout: Option<Duration>) -> Result<ureq::Agent> {
        let token_file = self.token_file.clone().or_else(token_file_from_env);
        server_agent(token_file.as_deref(), timeout)
    }
}

/// The agent for a command with no `--token-file` flag of its own.
pub fn server_agent_from_env() -> Result<ureq::Agent> {
    server_agent(token_file_from_env().as_deref(), None)
}

/// Read here rather than through clap's `env`, which rejects a variable that
/// is exported but empty instead of treating it as unset. Blank counts as
/// empty, as it does for the server URL.
fn token_file_from_env() -> Option<PathBuf> {
    std::env::var_os(TOKEN_FILE_ENV)
        .filter(|path| !path.as_encoded_bytes().trim_ascii().is_empty())
        .map(PathBuf::from)
}

/// The server's `{ error, message }` body; we surface only `message`.
#[derive(Debug, Deserialize)]
struct ServerError {
    message: String,
}

/// Read a 200 body as JSON, or turn a non-200 into the server's error message.
pub fn read_or_error<T: DeserializeOwned>(resp: ureq::http::Response<ureq::Body>) -> Result<T> {
    if resp.status() == StatusCode::OK {
        return resp
            .into_body()
            .read_json()
            .map_err(|_| anyhow::anyhow!("server returned malformed JSON"));
    }
    bail!("{}", error_message(resp))
}

/// The response when it is a 200; otherwise its error goes to stderr and the
/// caller has nothing left to do but exit non-zero.
pub fn ok_or_report(
    resp: ureq::http::Response<ureq::Body>,
) -> Option<ureq::http::Response<ureq::Body>> {
    if resp.status() == StatusCode::OK {
        return Some(resp);
    }
    eprintln!("error: {}", error_message(resp));
    None
}

/// What to tell the user about a non-200 response: the server's own `message`,
/// except for a missing or unknown token, where the server can only describe
/// the header it wanted and the fix is on this side.
pub fn error_message(resp: ureq::http::Response<ureq::Body>) -> String {
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        return format!(
            "the server did not accept this command's API token. Set `{ADMIN_TOKEN_ENV}` to a \
             token from the server's `api_tokens`, or `{TOKEN_FILE_ENV}` to a file holding one"
        );
    }
    resp.into_body().read_json::<ServerError>().map_or_else(
        |_| format!("server returned HTTP {}", status.as_u16()),
        |e| e.message,
    )
}

/// A client for a submilli-server that sends the API token, when there is one,
/// on every request. Non-2xx responses come back as responses rather than
/// errors, so the caller can show the server's `{ error, message }` body.
///
/// Only for requests to a submilli-server: the token must not reach the
/// release mirrors and third-party hosts other commands talk to.
fn server_agent(token_file: Option<&Path>, timeout: Option<Duration>) -> Result<ureq::Agent> {
    let mut config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(timeout);
    if let Some(authorization) = authorization_header(token_file)? {
        config = config.middleware(
            move |mut request: ureq::http::Request<ureq::SendBody>,
                  next: ureq::middleware::MiddlewareNext| {
                request
                    .headers_mut()
                    .insert(AUTHORIZATION, authorization.clone());
                next.handle(request)
            },
        );
    }
    Ok(config.build().into())
}

/// The `Authorization` header for the configured token, or `None` when no
/// token is configured. Errors name where the token was read from, never the
/// token.
fn authorization_header(token_file: Option<&Path>) -> Result<Option<HeaderValue>> {
    let Some((source, token)) = configured_token(token_file)? else {
        return Ok(None);
    };
    // Checked here rather than left to the transport, which fails the request
    // for a non-ASCII byte — a failure `server stop` would read as "no server".
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(unsendable(&source));
    }
    let mut value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| unsendable(&source))?;
    value.set_sensitive(true);
    Ok(Some(value))
}

/// The token with surrounding whitespace removed, and a description of where
/// it came from. The file wins over the variable.
fn configured_token(token_file: Option<&Path>) -> Result<Option<(String, String)>> {
    if let Some(path) = token_file {
        let token = std::fs::read_to_string(path)
            .with_context(|| format!("reading token file `{}`", path.display()))?;
        let token = token.trim();
        // Unlike an empty variable, an empty file the user pointed at is a
        // mistake worth reporting.
        if token.is_empty() {
            bail!("the token file `{}` is empty", path.display());
        }
        return Ok(Some((
            format!("token file `{}`", path.display()),
            token.to_owned(),
        )));
    }
    let Some(token) = std::env::var_os(ADMIN_TOKEN_ENV) else {
        return Ok(None);
    };
    let source = format!("`${ADMIN_TOKEN_ENV}` variable");
    // Not valid Unicode is not unset: sending nothing would turn a mangled
    // token into an unexplained 401.
    let token = token.into_string().map_err(|_| unsendable(&source))?;
    let token = token.trim();
    // Exported but empty counts as unset, like the other settings here.
    Ok((!token.is_empty()).then(|| (source, token.to_owned())))
}

fn unsendable(source: &str) -> anyhow::Error {
    anyhow::anyhow!("the token in the {source} contains a character that cannot be sent")
}

/// Where release downloads come from: the GitHub repository, or the mirror
/// named by `env_var`. Plain HTTP is accepted only for loopback so released
/// content cannot be replaced in transit.
pub fn release_source(env_var: &str) -> Result<String> {
    const DEFAULT: &str = "https://github.com/submilli/submilli-runtime";
    let source = std::env::var(env_var).unwrap_or_else(|_| DEFAULT.into());
    let url = url::Url::parse(&source).map_err(|_| anyhow::anyhow!("invalid {env_var}"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("{env_var} must use https");
    }
    Ok(source.trim_end_matches('/').to_owned())
}

/// Read a body into memory, refusing anything larger than `limit` bytes.
pub fn read_limited(response: ureq::http::Response<ureq::Body>, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("response exceeds {limit} bytes");
    }
    Ok(bytes)
}
