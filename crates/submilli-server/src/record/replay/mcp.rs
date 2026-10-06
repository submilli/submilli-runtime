//! The recorded-world MCP transport.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use base64::Engine as _;
use interpreter::runtime::mcp::request_digest;
use interpreter::runtime::{BodyCopy, McpCallError, McpOutcome, McpResponse, McpTransport};
use serde_json::Value;

use super::cassette::{Cassette, Entry, Kind, Unusable, mcp_key};

/// An [`McpTransport`] that answers from a recorded run: the next unused recording of the
/// same server, tool and arguments.
///
/// A recorded response is admitted through [`McpResponse::take`], the bounded path a live
/// response takes. A recorded failure is raised again from its kind.
pub struct RecordedMcpTransport {
    cassette: Arc<Cassette>,
}

impl RecordedMcpTransport {
    pub fn new(cassette: Arc<Cassette>) -> Self {
        Self { cassette }
    }
}

impl McpTransport for RecordedMcpTransport {
    fn call<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn Future<Output = McpOutcome> + Send + 'a>> {
        Box::pin(async move {
            let digest = request_digest(server, tool, args_json);
            let key = mcp_key(server, tool);
            match self.cassette.serve(Kind::Mcp, &key, &[digest], answer) {
                Ok(outcome) => outcome,
                Err(miss) => {
                    self.cassette.stop(miss).await;
                    McpOutcome {
                        // Fatal to the guest as well, in case the cancel were somehow not
                        // seen.
                        result: Err(McpCallError::Internal {
                            message: "no recorded response for this MCP call",
                        }),
                        received_bytes: 0,
                        parsed_bytes: 0,
                    }
                }
            }
        })
    }
}

fn answer(entry: &Entry) -> Result<McpOutcome, Unusable> {
    let response = entry
        .response
        .as_ref()
        .ok_or_else(|| Unusable::incomplete("the recorded call never finished"))?;
    if response.truncated {
        return Err(Unusable::incomplete(
            "the recorded response was cut by the recorder's caps",
        ));
    }
    if response.meta.is_object() {
        return Ok(McpOutcome {
            result: Err(failure(&response.meta)?),
            received_bytes: response.bytes,
            parsed_bytes: 0,
        });
    }
    let body = match &response.body {
        Some(BodyCopy::Text(text)) => text.clone().into_bytes(),
        Some(BodyCopy::Base64(data)) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Unusable::incomplete("the recorded response is unreadable"))?,
        None => {
            return Err(Unusable::incomplete(
                "the recorder kept the digest of the response, not its value",
            ));
        }
    };
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|_| Unusable::incomplete("the recorded response is unreadable"))?;
    Ok(McpOutcome {
        result: McpResponse::take(&mut value),
        received_bytes: response.bytes,
        parsed_bytes: body.len() as u64,
    })
}

/// The failure a recording kept, raised again from its kind.
fn failure(meta: &Value) -> Result<McpCallError, Unusable> {
    let kind = meta
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Unusable::failure("the recorded failure kept no kind"))?;
    let message = meta
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match kind {
        "response-too-large" => Ok(McpCallError::ResponseTooLarge),
        "auth-expired" => Ok(McpCallError::AuthExpired),
        "mcp" => Ok(McpCallError::Mcp { message }),
        "transport" => Ok(McpCallError::Transport(message)),
        "upstream" => {
            let status = meta
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .ok_or_else(|| Unusable::failure("the recorded upstream failure kept no status"))?;
            Ok(McpCallError::Upstream {
                status,
                body: message,
            })
        }
        other => Err(Unusable::failure(format!(
            "a recorded `{other}` failure cannot be raised again"
        ))),
    }
}
