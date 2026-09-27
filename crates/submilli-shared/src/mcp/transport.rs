//! Outbound MCP `streamable_http` transport: the runtime side of an
//! `@mcp/<server>.<tool>(...)` call.
//!
//! Implements [`interpreter::McpTransport`]: resolve auth (static `${secrets.X}`
//! headers or an OAuth bearer token), then perform a JSON-RPC `tools/call` over
//! rmcp's streamable-HTTP client — the same client discovery uses, so the MCP
//! `initialize`/session handshake and SSE framing come for free. OAuth calls retry
//! **once** after forcing a token refresh; `invalid_grant` demotes the blueprint to
//! PENDING (inside [`OAuthTokenManager`]) and surfaces as
//! [`McpCallError::AuthExpired`].
//!
//! A fresh rmcp session is opened per call. Caching a session per
//! `(blueprint, server)` is a future optimization; correctness comes first.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::host::EnvFileSecretResolver;
use http::{HeaderName, HeaderValue};
use interpreter::runtime::{McpCallError, McpTransport};
use interpreter::stdlib::http::{NetworkPolicy, describe_error_chain};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo, Content};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Map, Value};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, McpAuth, McpServer, interpolate};

use crate::mcp_token::{McpTokenError, OAuthTokenManager};
use crate::secret_store::SecretStore;

/// Bounds authentication, connection, the tool response, and teardown together.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// The outbound MCP transport bound to one blueprint. Built per execute and set on
/// the store; `call` runs once per `@mcp/<server>.<tool>` invocation.
pub struct StreamableHttpTransport {
    blueprint_name: String,
    blueprint: Arc<Blueprint>,
    oauth: Option<Arc<OAuthTokenManager>>,
    secret_store: Option<Arc<dyn SecretStore>>,
    harness_secrets: Arc<HarnessSecretBindings>,
    /// The server's outbound-address policy; an MCP server's `url` is judged
    /// like any other outbound destination.
    policy: Arc<NetworkPolicy>,
}

impl StreamableHttpTransport {
    pub fn new(
        blueprint_name: String,
        blueprint: Arc<Blueprint>,
        oauth: Option<Arc<OAuthTokenManager>>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
    ) -> Self {
        Self {
            blueprint_name,
            blueprint,
            oauth,
            secret_store,
            harness_secrets: Arc::new(HarnessSecretBindings::new()),
            policy,
        }
    }

    pub fn with_harness_secrets(mut self, secrets: Arc<HarnessSecretBindings>) -> Self {
        self.harness_secrets = secrets;
        self
    }

    /// Resolve a static-`headers:` server's `${secrets.X}` placeholders into rmcp
    /// custom headers.
    async fn static_headers(
        &self,
        server: &McpServer,
    ) -> Result<HashMap<HeaderName, HeaderValue>, McpCallError> {
        let resolver = EnvFileSecretResolver::with_harness(
            self.secret_store.clone(),
            Arc::clone(&self.harness_secrets),
        );
        let mut headers = HashMap::with_capacity(server.headers.len());
        for (name, value) in &server.headers {
            let resolved = interpolate(value, &self.blueprint, &resolver)
                .await
                .map_err(|e| McpCallError::Transport(e.to_string()))?;
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| McpCallError::Transport(e.to_string()))?;
            let header_value = HeaderValue::from_str(&resolved)
                .map_err(|e| McpCallError::Transport(e.to_string()))?;
            headers.insert(header_name, header_value);
        }
        Ok(headers)
    }

    /// One `tools/call` round-trip over a fresh rmcp streamable-HTTP session.
    async fn call_once(
        &self,
        server: &McpServer,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        tool: &str,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpCallError> {
        self.policy
            .check_url(server.url.as_str())
            .await
            .map_err(McpCallError::Transport)?;
        let mut config = StreamableHttpClientTransportConfig::with_uri(server.url.as_str());
        config.auth_header = auth_header;
        config.custom_headers = custom_headers;
        // Tolerate stateless servers (per-request auth, no Mcp-Session-Id) — see
        // the discovery path for the rationale. Both stateful and stateless work.
        config.allow_stateless = true;
        let transport =
            StreamableHttpClientTransport::with_client(policy_http_client(&self.policy), config);
        let client = ClientInfo::default()
            .serve(transport)
            .await
            .map_err(|e| McpCallError::Transport(describe_error_chain(&e)))?;
        let param = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = client.call_tool(param).await;
        // Best-effort teardown regardless of the call outcome.
        let _ = client.cancel().await;
        result.map_err(|e| McpCallError::Transport(e.to_string()))
    }

    /// OAuth call path: mint a bearer token, call, and on failure force a refresh
    /// and retry exactly once. (rmcp doesn't surface the raw `401`, so any error on
    /// the first attempt triggers one refresh-and-retry; a genuine non-auth failure
    /// simply fails the retry too.)
    async fn call_oauth(
        &self,
        server_name: &str,
        server: &McpServer,
        tool: &str,
        arguments: &Map<String, Value>,
    ) -> Result<CallToolResult, McpCallError> {
        let oauth = self.oauth.as_ref().ok_or_else(|| {
            McpCallError::Transport(format!(
                "OAuth server '{server_name}' needs a secret store, none configured"
            ))
        })?;
        let token = oauth
            .access_token(&self.blueprint_name, server_name)
            .await
            .map_err(map_token_err)?;
        // `auth_header` is the raw token — rmcp's transport prepends "Bearer ".
        // (Pre-formatting "Bearer {t}" here would double it → a malformed header.)
        let first = self
            .call_once(
                server,
                Some(token.clone()),
                HashMap::new(),
                tool,
                arguments.clone(),
            )
            .await;
        if first.is_ok() {
            return first;
        }

        // First attempt failed: force a refresh and retry exactly once.
        let fresh = match oauth
            .force_refresh(&self.blueprint_name, server_name, &token)
            .await
        {
            Ok(t) => t,
            Err(McpTokenError::AuthExpired) => return Err(McpCallError::AuthExpired),
            Err(e) => return Err(map_token_err(e)),
        };
        self.call_once(server, Some(fresh), HashMap::new(), tool, arguments.clone())
            .await
    }

    async fn call_tool(
        &self,
        server_name: &str,
        tool: &str,
        args_json: &str,
    ) -> Result<Value, McpCallError> {
        let server = self.blueprint.mcp.get(server_name).ok_or_else(|| {
            McpCallError::Transport(format!("server '{server_name}' is not declared"))
        })?;
        // `streamable_http` (and its `sse` alias) only; `stdio` is deferred.
        if server.transport == "stdio" {
            return Err(McpCallError::Transport(
                "stdio transport is deferred to a later release".to_string(),
            ));
        }

        // The args object is a JSON object; an empty/non-object arg → `{}`.
        let arguments = match serde_json::from_str::<Value>(args_json) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };

        let result = match &server.auth {
            Some(McpAuth::Oauth2 { .. }) => {
                self.call_oauth(server_name, server, tool, &arguments)
                    .await?
            }
            None => {
                let headers = self.static_headers(server).await?;
                self.call_once(server, None, headers, tool, arguments)
                    .await?
            }
        };
        interpret(result)
    }
}

impl McpTransport for StreamableHttpTransport {
    fn call<'a>(
        &'a self,
        server_name: &'a str,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, McpCallError>> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::timeout(CALL_TIMEOUT, self.call_tool(server_name, tool, args_json))
                .await
                .map_err(|_| {
                    McpCallError::Transport(format!(
                        "MCP tool '{server_name}/{tool}' timed out after {} seconds",
                        CALL_TIMEOUT.as_secs()
                    ))
                })?
        })
    }
}

/// Map a `CallToolResult` into the value the script receives, or a typed error.
fn interpret(result: CallToolResult) -> Result<Value, McpCallError> {
    if result.is_error.unwrap_or(false) {
        return Err(McpCallError::Mcp {
            message: content_text(&result.content),
        });
    }
    if let Some(structured) = result.structured_content {
        return Ok(structured);
    }
    Ok(content_to_value(&result.content))
}

/// Turn the tool's content blocks into a value: a single text block parsed as JSON
/// (so a typed tool's JSON text validates), or its raw string when it isn't JSON.
fn content_to_value(content: &[Content]) -> Value {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect();
    match texts.as_slice() {
        [] => Value::Null,
        [single] => {
            serde_json::from_str(single).unwrap_or_else(|_| Value::String((*single).to_string()))
        }
        many => Value::String(many.join("\n")),
    }
}

/// Join the text of a tool result's content blocks (for `isError` messages).
fn content_text(content: &[Content]) -> String {
    let text = content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        "MCP tool reported an error".to_string()
    } else {
        text
    }
}

fn map_token_err(err: McpTokenError) -> McpCallError {
    match err {
        McpTokenError::AuthExpired => McpCallError::AuthExpired,
        McpTokenError::Upstream { status, body } => McpCallError::Upstream { status, body },
        other => McpCallError::Transport(other.to_string()),
    }
}

/// The reqwest client rmcp drives, built on the policy's resolver. Idle pooling
/// is off for the same reason rmcp's own default disables it: a stall on
/// connection reuse after an unconsumed body.
pub(crate) fn policy_http_client(policy: &Arc<NetworkPolicy>) -> reqwest::Client {
    policy
        .client_builder()
        .pool_max_idle_per_host(0)
        .build()
        .expect("the reqwest client builds from static configuration")
}

#[cfg(test)]
mod tests {
    #[tokio::test(start_paused = true)]
    async fn hung_server_call_has_a_deadline() {
        use std::time::Duration;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let bp = parse(&format!(
            "name: bp\nmcp:\n  local:\n    url: http://{address}/mcp\n"
        ))
        .unwrap();
        let transport = StreamableHttpTransport::new(
            "bp".into(),
            Arc::new(bp),
            None,
            None,
            Arc::new(NetworkPolicy::allow_all()),
        );
        let result =
            tokio::time::timeout(Duration::from_secs(61), transport.call("local", "t", "{}")).await;
        let error = result
            .expect("MCP call must expire before the enclosing program deadline")
            .unwrap_err();
        assert!(matches!(error, McpCallError::Transport(message) if message.contains("timed out")));
    }

    use std::sync::Arc;

    use rmcp::model::CallToolResult;
    use serde_json::json;
    use submilli_blueprint::parse;

    use super::*;

    fn text_result(text: &str, is_error: bool) -> CallToolResult {
        let content = vec![Content::text(text.to_string())];
        if is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
    }

    #[test]
    fn structured_content_is_preferred() {
        let mut r = CallToolResult::success(vec![Content::text("ignored".to_string())]);
        r.structured_content = Some(json!({ "id": 7 }));
        assert_eq!(interpret(r).unwrap(), json!({ "id": 7 }));
    }

    #[test]
    fn json_text_content_is_parsed() {
        let r = text_result(r#"{"id":7}"#, false);
        assert_eq!(interpret(r).unwrap(), json!({ "id": 7 }));
    }

    #[test]
    fn plain_text_content_stays_a_string() {
        let r = text_result("ok", false);
        assert_eq!(interpret(r).unwrap(), json!("ok"));
    }

    #[test]
    fn is_error_maps_to_mcp() {
        let r = text_result("tool failed", true);
        assert!(
            matches!(interpret(r), Err(McpCallError::Mcp { message }) if message == "tool failed")
        );
    }

    fn static_transport() -> StreamableHttpTransport {
        let bp = parse("name: bp\nmcp:\n  linear:\n    url: https://x/mcp\n").unwrap();
        StreamableHttpTransport::new(
            "bp".into(),
            Arc::new(bp),
            None,
            None,
            Arc::new(NetworkPolicy::allow_all()),
        )
    }

    /// An MCP server's `url` is an outbound destination like any other: under
    /// deny-private a loopback server is refused, by the literal-IP check or at
    /// resolution, and the error names the policy.
    #[tokio::test]
    async fn a_private_mcp_server_is_blocked_by_the_network_policy() {
        for url in ["http://127.0.0.1:1/mcp", "http://localhost:1/mcp"] {
            let bp = parse(&format!("name: bp\nmcp:\n  local:\n    url: {url}\n")).unwrap();
            let t = StreamableHttpTransport::new(
                "bp".into(),
                Arc::new(bp),
                None,
                None,
                Arc::new(NetworkPolicy::deny_private()),
            );
            match t.call("local", "t", "{}").await {
                Err(McpCallError::Transport(message)) => assert!(
                    message.contains("blocked by network policy"),
                    "{url}: expected the policy reason, got: {message}"
                ),
                other => panic!("{url}: expected a transport error, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn stdio_transport_is_rejected() {
        let mut bp = parse("name: bp\nmcp:\n  linear:\n    url: https://x/mcp\n").unwrap();
        bp.mcp.get_mut("linear").unwrap().transport = "stdio".to_string();
        let t = StreamableHttpTransport::new(
            "bp".into(),
            Arc::new(bp),
            None,
            None,
            Arc::new(NetworkPolicy::allow_all()),
        );
        assert!(matches!(
            t.call("linear", "t", "{}").await,
            Err(McpCallError::Transport(_))
        ));
    }

    #[tokio::test]
    async fn unknown_server_errors() {
        let t = static_transport();
        assert!(matches!(
            t.call("ghost", "t", "{}").await,
            Err(McpCallError::Transport(_))
        ));
    }
}
