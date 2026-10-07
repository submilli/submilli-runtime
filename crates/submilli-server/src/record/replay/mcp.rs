//! The recorded-world MCP transport.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::runtime::mcp::request_digest;
use interpreter::runtime::{McpCallError, McpOutcome, McpResponse, McpTransport};
use serde_json::Value;
use submilli_blueprint::Blueprint;
use submilli_shared::mcp::transport::callable_server;

use super::cassette::{Cassette, Entry, Kind, Miss, Unusable, decoded_body, mcp_key};

/// An [`McpTransport`] that answers from a recorded run: the next unused recording of the
/// same server, tool and arguments.
///
/// A server the current blueprint does not declare, or declares over `stdio`, is refused
/// as the live transport refuses it, whatever was recorded: the recording answers a call,
/// not a declaration.
///
/// A recorded response is admitted through [`McpResponse::take`], the bounded path a live
/// response takes. A recorded failure is raised again from its kind.
///
/// With [`with_live`](Self::with_live), a call the recording cannot answer goes to the live
/// transport instead of stopping the run.
pub struct RecordedMcpTransport {
    cassette: Arc<Cassette>,
    blueprint: Arc<Blueprint>,
    live: Option<Arc<dyn McpTransport>>,
}

impl RecordedMcpTransport {
    /// Answers for the servers `blueprint` declares.
    pub fn new(cassette: Arc<Cassette>, blueprint: Arc<Blueprint>) -> Self {
        Self {
            cassette,
            blueprint,
            live: None,
        }
    }

    #[must_use]
    pub fn with_live(mut self, live: Arc<dyn McpTransport>) -> Self {
        self.live = Some(live);
        self
    }

    /// A call with nothing recorded: sent live when this transport may, else it stops the run.
    async fn unanswered(
        &self,
        miss: Miss,
        server: &str,
        tool: &str,
        args_json: &str,
    ) -> McpOutcome {
        if let Some(live) = &self.live {
            self.cassette.went_live(miss);
            return live.call(server, tool, args_json).await;
        }
        self.cassette.stop(miss).await;
        McpOutcome {
            // Fatal to the guest as well, in case the cancel were somehow not seen.
            result: Err(McpCallError::Internal {
                message: "no recorded response for this MCP call",
            }),
            received_bytes: 0,
            parsed_bytes: 0,
        }
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
            if let Err(refused) = callable_server(&self.blueprint, server) {
                return McpOutcome {
                    result: Err(refused),
                    received_bytes: 0,
                    parsed_bytes: 0,
                };
            }
            let digest = request_digest(server, tool, args_json);
            let key = mcp_key(server, tool);
            match self.cassette.serve(Kind::Mcp, &key, &[digest], answer) {
                Ok(outcome) => outcome,
                Err(miss) => self.unanswered(miss, server, tool, args_json).await,
            }
        })
    }
}

fn answer(entry: &Entry) -> Result<McpOutcome, Unusable> {
    let response = entry.finished_response()?;
    if response.meta.is_object() {
        return Ok(McpOutcome {
            result: Err(failure(&response.meta)?),
            received_bytes: response.bytes,
            parsed_bytes: 0,
        });
    }
    let body = decoded_body(response, "value")?;
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|_| Unusable::incomplete("the recorded response is unreadable"))?;
    Ok(McpOutcome {
        result: McpResponse::take(&mut value),
        received_bytes: response.bytes,
        parsed_bytes: body.len() as u64,
    })
}

/// The failure a recording kept, raised again from its kind: only an answer from outside,
/// the server's or the wire's. Every `McpCallError::record()` kind:
///
/// | kind                 | class                                                     |
/// |----------------------|-----------------------------------------------------------|
/// | `mcp`                | outside: the server's tool or JSON-RPC error; served      |
/// | `upstream`           | outside: the token endpoint's non-2xx answer; served, but a 401 or 403 is the credential it ran with, so a miss |
/// | `response-too-large` | outside: the response passed the fixed bound; served      |
/// | `transport`          | outside: the connection or token endpoint failed; served  |
/// | `local`              | local: credentials, network policy, header config; miss   |
/// | `auth-expired`       | local: the credential's state, today's decides; miss      |
/// | `internal`           | local: host setup failure; miss                           |
///
/// A kind not listed, or none, is a miss too. `upstream` is only ever the token endpoint's
/// answer, never the server's own status. `transport` is outside only because the
/// transport records its local refusals (credential state, network policy, a secret that
/// will not resolve) as `local`.
fn failure(meta: &Value) -> Result<McpCallError, Unusable> {
    let kind = meta.get("kind").and_then(Value::as_str);
    let message = meta
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match kind {
        Some("response-too-large") => Ok(McpCallError::ResponseTooLarge),
        Some("mcp") => Ok(McpCallError::Mcp { message }),
        Some("transport") => Ok(McpCallError::Transport(message)),
        Some("upstream") => {
            let status = meta
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .ok_or_else(|| Unusable::failure("the recorded upstream failure kept no status"))?;
            if matches!(status, 401 | 403) {
                return Err(Unusable::decided_today(kind));
            }
            Ok(McpCallError::Upstream {
                status,
                body: message,
            })
        }
        other => Err(Unusable::decided_today(other)),
    }
}
