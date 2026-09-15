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
