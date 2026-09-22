//! Shared HTTP plumbing for commands that talk to a server: a ureq client that
//! surfaces non-2xx bodies instead of erroring, and the `{ error, message }`
//! envelope both servers use for rejections.

use anyhow::{Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// The server's `{ error, message }` body; we surface only `message`.
#[derive(Debug, Deserialize)]
pub struct ServerError {
    pub message: String,
}

pub fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

/// Read a 200 body as JSON, or turn a non-200 into the server's error message.
pub fn read_or_error<T: DeserializeOwned>(resp: ureq::http::Response<ureq::Body>) -> Result<T> {
    let status = resp.status().as_u16();
    if status == 200 {
        return resp
            .into_body()
            .read_json()
            .map_err(|_| anyhow::anyhow!("server returned malformed JSON"));
    }
    let message = resp
        .into_body()
        .read_json::<ServerError>()
        .map_or_else(|_| format!("server returned HTTP {status}"), |e| e.message);
    bail!("{message}")
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
