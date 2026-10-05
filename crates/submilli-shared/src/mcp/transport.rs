//! Outbound MCP `streamable_http` transport: the runtime side of an
//! `@mcp/<server>.<tool>(...)` call.
//!
//! Implements [`interpreter::McpTransport`]: resolve auth (static `${secrets.X}`
//! headers or an OAuth bearer token), then perform a JSON-RPC `tools/call` over
//! rmcp's streamable-HTTP client — the same client discovery uses, so the MCP
//! `initialize`/session handshake and SSE framing come for free. OAuth calls retry
//! **once** after forcing a token refresh. [`OAuthTokenManager`] retries
//! `invalid_grant` briefly, then surfaces [`McpCallError::AuthExpired`] while
//! retaining the credential for a later attempt.
//!
//! One rmcp session per server is opened on the first call and kept for the rest
//! of the execute, so a server that holds state in its session (a browser page,
//! a cursor) sees one program's calls as one conversation. A call that fails or
//! times out drops the session, and the next call opens a new one.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::bounded_client::{BoundedClient, ResponseBudget};
use super::draining_transport::{DrainingTransport, PendingTransport};
use crate::host::BlueprintSecretResolver;
use http::{HeaderName, HeaderValue};
use interpreter::runtime::{McpCallError, McpOutcome, McpResponse, McpTransport};
use interpreter::stdlib::http::{NetworkPolicy, describe_error_chain};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo, Content};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::Transport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Map, Value};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, McpAuth, McpServer, interpolate};

use crate::mcp_token::{McpTokenError, OAuthTokenManager};
use crate::secret_store::SecretStore;

pub(crate) type PolicyClientFactory =
    dyn Fn(&Arc<NetworkPolicy>) -> Result<reqwest::Client, reqwest::Error> + Send + Sync;

/// Bounds authentication, connection, and the tool response together.
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
    client_factory: Arc<PolicyClientFactory>,
    /// The sessions this execute has opened, by server name. Held across a call,
    /// which also keeps two calls from interleaving on one session.
    sessions: tokio::sync::Mutex<HashMap<String, McpSession>>,
}

#[derive(Default)]
struct SessionAuth {
    bearer: Option<String>,
    headers: HashMap<HeaderName, HeaderValue>,
}

struct McpSession {
    service: RunningService<RoleClient, ClientInfo>,
    budget: Arc<ResponseBudget>,
}

/// Survives cancellation of the call future and retains every attempt's work.
#[derive(Default)]
struct CallWork {
    received_bytes: u64,
    embedded_parse_bytes: u64,
    budget: Option<Arc<ResponseBudget>>,
    pending_transport: Option<PendingTransport>,
    cleanup: Option<Pin<Box<dyn std::future::Future<Output = ()> + Send>>>,
    request_timeout: Option<Duration>,
}

impl CallWork {
    fn retain_cleanup(&mut self, cleanup: impl std::future::Future<Output = ()> + Send + 'static) {
        let previous = self.cleanup.take();
        let budget = self.budget.clone();
        self.cleanup = Some(Box::pin(async move {
            if let Some(previous) = previous {
                previous.await;
            }
            cleanup.await;
            // rmcp aborts its SSE JoinSet asynchronously; parent shutdown alone
            // does not ensure those streams have stopped recording response bytes.
            if let Some(budget) = budget {
                budget.wait_idle().await;
            }
        }));
    }

    async fn drain_connection(&mut self) {
        if let Some(pending) = self.pending_transport.take() {
            self.retain_cleanup(async move {
                if let Ok(mut transport) = pending.await {
                    let _ = transport.close().await;
                }
            });
        }
        // Await by reference: cancelling this await leaves the join future and
        // its owned service/transport in CallWork for the outer cleanup pass.
        if let Some(cleanup) = self.cleanup.as_mut() {
            cleanup.await;
        }
        self.cleanup = None;
    }

    fn attach(&mut self, budget: Arc<ResponseBudget>) {
        self.drain();
        self.budget = Some(budget);
    }

    fn drain(&mut self) {
        if let Some(budget) = self.budget.take() {
            self.received_bytes = self.received_bytes.saturating_add(budget.take_received());
        }
    }

    fn finish(mut self, result: Result<McpResponse, McpCallError>) -> McpOutcome {
        let result = match self.budget.as_ref().and_then(|budget| budget.error()) {
            Some(error) => Err(error),
            None => result,
        };
        self.drain();
        McpOutcome {
            result,
            received_bytes: self.received_bytes,
            parsed_bytes: self
                .received_bytes
                .saturating_add(self.embedded_parse_bytes),
        }
    }
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
            client_factory: Arc::new(policy_http_client),
            sessions: tokio::sync::Mutex::new(HashMap::new()),
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
        let resolver = BlueprintSecretResolver::with_harness(
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

    /// One `tools/call` round-trip over this execute's session with the server,
    /// opening it on the first call. The credentials are those of the call that
    /// opened the session.
    async fn call_once(
        &self,
        server_name: &str,
        server: &McpServer,
        auth: SessionAuth,
        tool: &str,
        arguments: Map<String, Value>,
        work: &mut CallWork,
    ) -> Result<CallToolResult, McpCallError> {
        work.drain_connection().await;
        let mut sessions = self.sessions.lock().await;
        if !sessions.contains_key(server_name) {
            let session = self
                .connect(server, auth.bearer, auth.headers, work)
                .await?;
            sessions.insert(server_name.to_string(), session);
        }
        let Some(session) = sessions.get(server_name) else {
            return Err(McpCallError::Internal {
                message: "MCP session is missing after initialization",
            });
        };
        work.attach(session.budget.clone());
        let param = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = tokio::select! {
            result = session.service.call_tool(param) => {
                if let Some(error) = session.budget.error() { Err(error) }
                else { result.map_err(|error| McpCallError::Transport(error.to_string())) }
            }
            error = session.budget.wait() => Err(error),
        };
        if result.is_err()
            && let Some(broken) = sessions.remove(server_name)
        {
            work.retain_cleanup(async move {
                let _ = broken.service.cancel().await;
            });
        }
        result
    }

    async fn connect(
        &self,
        server: &McpServer,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        work: &mut CallWork,
    ) -> Result<McpSession, McpCallError> {
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
        let client = BoundedClient::new(
            (self.client_factory)(&self.policy).map_err(client_initialization_failure)?,
        )
        .with_timeout(work.request_timeout.unwrap_or(CALL_TIMEOUT));
        let budget = client.budget.clone();
        work.attach(budget.clone());
        let (transport, pending) =
            DrainingTransport::new(StreamableHttpClientTransport::with_client(client, config));
        work.pending_transport = Some(pending);
        let service = tokio::select! {
            result = ClientInfo::default().serve(transport) => {
                if let Some(error) = budget.error() { return Err(error); }
                result.map_err(|error| McpCallError::Transport(describe_error_chain(&error)))?
            }
            error = budget.wait() => return Err(error),
        };
        work.pending_transport = None;
        Ok(McpSession { service, budget })
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
        work: &mut CallWork,
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
                server_name,
                server,
                SessionAuth {
                    bearer: Some(token.clone()),
                    ..SessionAuth::default()
                },
                tool,
                arguments.clone(),
                work,
            )
            .await;
        if first.is_err() {
            work.drain_connection().await;
            if let Some(error) = work.budget.as_ref().and_then(|budget| budget.error()) {
                return Err(error);
            }
        }
        if first.is_ok()
            || matches!(
                first,
                Err(McpCallError::ResponseTooLarge | McpCallError::Internal { .. })
            )
        {
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
        self.call_once(
            server_name,
            server,
            SessionAuth {
                bearer: Some(fresh),
                ..SessionAuth::default()
            },
            tool,
            arguments.clone(),
            work,
        )
        .await
    }

    async fn call_tool(
        &self,
        server_name: &str,
        tool: &str,
        args_json: &str,
        work: &mut CallWork,
    ) -> Result<McpResponse, McpCallError> {
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
                self.call_oauth(server_name, server, tool, &arguments, work)
                    .await?
            }
            None => {
                let headers = self.static_headers(server).await?;
                self.call_once(
                    server_name,
                    server,
                    SessionAuth {
                        headers,
                        ..SessionAuth::default()
                    },
                    tool,
                    arguments,
                    work,
                )
                .await?
            }
        };
        let mut value = interpret(result, &mut work.embedded_parse_bytes)?;
        McpResponse::take(&mut value)
    }
}

impl StreamableHttpTransport {
    async fn call_with_timeout(
        &self,
        server_name: &str,
        tool: &str,
        args_json: &str,
        timeout: Duration,
    ) -> McpOutcome {
        let mut work = CallWork {
            request_timeout: Some(timeout),
            ..CallWork::default()
        };
        let call = self.call_tool(server_name, tool, args_json, &mut work);
        let result = if let Ok(result) = tokio::time::timeout(timeout, call).await {
            result
        } else {
            // Drain the interrupted service before collecting its final counters.
            if let Some(session) = self.sessions.lock().await.remove(server_name) {
                work.retain_cleanup(async move {
                    let _ = session.service.cancel().await;
                });
            }
            Err(McpCallError::Transport(format!(
                "MCP tool '{server_name}/{tool}' timed out after {} seconds",
                timeout.as_secs()
            )))
        };
        work.drain_connection().await;
        work.finish(result)
    }
}

impl McpTransport for StreamableHttpTransport {
    fn call<'a>(
        &'a self,
        server_name: &'a str,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = McpOutcome> + Send + 'a>> {
        Box::pin(self.call_with_timeout(server_name, tool, args_json, CALL_TIMEOUT))
    }
}

/// Map a `CallToolResult` into the value the script receives, or a typed error.
fn interpret(result: CallToolResult, parsed_bytes: &mut u64) -> Result<Value, McpCallError> {
    if result.is_error.unwrap_or(false) {
        return Err(McpCallError::Mcp {
            message: content_text(&result.content),
        });
    }
    if let Some(structured) = result.structured_content {
        return Ok(structured);
    }
    Ok(content_to_value(&result.content, parsed_bytes))
}

/// Turn the tool's content blocks into a value: a single text block parsed as JSON
/// (so a typed tool's JSON text validates), or its raw string when it isn't JSON.
fn content_to_value(content: &[Content], parsed_bytes: &mut u64) -> Value {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect();
    match texts.as_slice() {
        [] => Value::Null,
        [single] => {
            *parsed_bytes = parsed_bytes.saturating_add(single.len() as u64);
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
pub(crate) fn policy_http_client(
    policy: &Arc<NetworkPolicy>,
) -> Result<reqwest::Client, reqwest::Error> {
    policy.client_builder().pool_max_idle_per_host(0).build()
}

fn client_initialization_failure(error: reqwest::Error) -> McpCallError {
    tracing::error!(error = ?error, "MCP HTTP client initialization failed");
    McpCallError::Internal {
        message: "MCP HTTP client initialization failed",
    }
}

#[cfg(test)]
mod tests {
    fn failing_transport() -> (StreamableHttpTransport, Arc<std::sync::atomic::AtomicUsize>) {
        let bp = parse("name: bp\nmcp:\n  local:\n    url: http://127.0.0.1:1/mcp\n").unwrap();
        let mut transport = StreamableHttpTransport::new(
            "bp".into(),
            Arc::new(bp),
            None,
            None,
            Arc::new(NetworkPolicy::allow_all()),
        );
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        transport.client_factory = {
            let attempts = attempts.clone();
            Arc::new(move |_| {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                reqwest::Client::builder()
                    .user_agent("invalid\nheader")
                    .build()
            })
        };
        (transport, attempts)
    }

    #[tokio::test]
    async fn client_initialization_failure_is_internal_and_has_no_pending_work() {
        let (transport, attempts) = failing_transport();
        let outcome = transport.call("local", "raw", "{}").await;
        assert!(matches!(
            outcome.result,
            Err(McpCallError::Internal {
                message: "MCP HTTP client initialization failed"
            })
        ));
        assert_eq!(outcome.received_bytes, 0);
        assert!(transport.sessions.lock().await.is_empty());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn internal_client_failure_does_not_refresh_oauth_or_retry() {
        use crate::mcp_auth::{OAuthCredential, write_credential};
        use crate::secret_store::PlaintextFileSecretStore;
        let (mut transport, attempts) = failing_transport();
        Arc::make_mut(&mut transport.blueprint)
            .mcp
            .get_mut("local")
            .unwrap()
            .auth = Some(McpAuth::Oauth2 {
            client_id: None,
            authorization_endpoint: None,
            token_endpoint: None,
            scopes: Vec::new(),
        });
        let root = tempfile::tempdir().unwrap();
        let store: Arc<dyn SecretStore> =
            Arc::new(PlaintextFileSecretStore::open(root.path().join("secrets")).unwrap());
        write_credential(
            "bp",
            "local",
            &OAuthCredential {
                refresh_token: None,
                access_token: Some("static".into()),
                client_id: None,
                token_endpoint: None,
                scopes: Vec::new(),
            },
            &store,
        )
        .await
        .unwrap();
        transport.oauth = Some(Arc::new(OAuthTokenManager::new(
            store,
            Arc::new(interpreter::runtime::ReqwestHttpClient::new(
                transport.policy.clone(),
            )),
            Arc::new(Vec::new()),
        )));
        assert!(matches!(
            transport.call("local", "raw", "{}").await.result,
            Err(McpCallError::Internal { .. })
        ));
        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "an OAuth retry would construct a second client"
        );
    }

    #[tokio::test]
    async fn late_internal_response_failure_overrides_timeout_after_cleanup() {
        let budget = Arc::new(ResponseBudget::default());
        let mut work = CallWork::default();
        work.attach(budget.clone());
        work.retain_cleanup(async move {
            budget.inject_allocation_failure();
        });
        work.drain_connection().await;
        let outcome = work.finish(Err(McpCallError::Transport("timed out".into())));
        assert!(matches!(outcome.result, Err(McpCallError::Internal { .. })));
    }

    #[tokio::test]
    async fn final_accounting_waits_for_detached_response_streams() {
        let budget = Arc::new(ResponseBudget::default());
        let activity = budget.activity();
        let mut work = CallWork::default();
        work.attach(budget.clone());
        work.retain_cleanup(async {});
        assert!(
            tokio::time::timeout(Duration::ZERO, work.drain_connection())
                .await
                .is_err()
        );
        budget.record(23);
        drop(activity);
        work.drain_connection().await;
        let outcome = work.finish(Err(McpCallError::Transport("closed".into())));
        assert_eq!(outcome.received_bytes, 23);
    }

    #[tokio::test]
    async fn cancelled_teardown_is_resumed_before_final_accounting() {
        let budget = Arc::new(ResponseBudget::default());
        let mut work = CallWork::default();
        work.attach(budget.clone());
        let (started, mut start) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        work.retain_cleanup(async move {
            started.send(()).unwrap();
            wait.await.unwrap();
            budget.record(17);
        });
        assert!(
            tokio::time::timeout(Duration::ZERO, work.drain_connection())
                .await
                .is_err()
        );
        assert_eq!(start.try_recv(), Ok(()));
        assert!(work.cleanup.is_some());
        release.send(()).unwrap();
        work.drain_connection().await;
        assert!(work.cleanup.is_none());
        let outcome = work.finish(Err(McpCallError::Transport("cancelled".into())));
        assert_eq!(outcome.received_bytes, 17);
        assert_eq!(outcome.parsed_bytes, 17);
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[tokio::test]
    async fn partial_initialize_response_is_accounted_after_timeout_and_cleanup() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        const BODY: &[u8] = b"{\"jsonrpc\":\"2.0\",";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 512\r\n\r\n").await.unwrap();
            stream.write_all(BODY).await.unwrap();
            // EOF demonstrates the pending response was cancelled before return.
            loop {
                if stream.read(&mut request).await.unwrap_or(0) == 0 {
                    break;
                }
            }
        });
        let blueprint = parse(&format!(
            "name: bp\nmcp:\n  local:\n    url: http://{address}/mcp\n"
        ))
        .unwrap();
        let transport = StreamableHttpTransport::new(
            "bp".into(),
            Arc::new(blueprint),
            None,
            None,
            Arc::new(NetworkPolicy::allow_all()),
        );
        let outcome = transport
            .call_with_timeout("local", "t", "{}", Duration::from_millis(500))
            .await;
        assert!(outcome.result.is_err());
        assert_eq!(outcome.received_bytes, BODY.len() as u64);
        assert_eq!(outcome.parsed_bytes, BODY.len() as u64);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert!(transport.sessions.lock().await.is_empty());
    }

    #[test]
    fn outcome_preserves_work_across_attempts_and_separates_embedded_parsing() {
        let first = Arc::new(ResponseBudget::default());
        let second = Arc::new(ResponseBudget::default());
        let mut work = CallWork::default();
        work.attach(first.clone());
        first.record(32);
        work.attach(second.clone());
        second.record(64);
        work.embedded_parse_bytes = 17;
        let outcome = work.finish(Err(McpCallError::AuthExpired));
        assert_eq!(outcome.received_bytes, 96);
        assert_eq!(outcome.parsed_bytes, 113);
        assert_eq!(first.take_received(), 0);
        assert_eq!(second.take_received(), 0);
        assert!(matches!(outcome.result, Err(McpCallError::AuthExpired)));
    }

    #[test]
    fn embedded_text_counts_successful_and_unsuccessful_parse_attempts() {
        for text in ["{\"id\":7}", "plain text"] {
            let mut parsed = 0;
            interpret(text_result(text, false), &mut parsed).unwrap();
            assert_eq!(parsed, text.len() as u64);
        }
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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
        let result = tokio::time::timeout(Duration::from_secs(61), async {
            transport.call("local", "t", "{}").await.result
        })
        .await;
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
        assert_eq!(interpret(r, &mut 0).unwrap(), json!({ "id": 7 }));
    }

    #[test]
    fn json_text_content_is_parsed() {
        let r = text_result(r#"{"id":7}"#, false);
        assert_eq!(interpret(r, &mut 0).unwrap(), json!({ "id": 7 }));
    }

    #[test]
    fn plain_text_content_stays_a_string() {
        let r = text_result("ok", false);
        assert_eq!(interpret(r, &mut 0).unwrap(), json!("ok"));
    }

    #[test]
    fn is_error_maps_to_mcp() {
        let r = text_result("tool failed", true);
        assert!(
            matches!(interpret(r, &mut 0), Err(McpCallError::Mcp { message, .. }) if message == "tool failed")
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
            match t.call("local", "t", "{}").await.result {
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
            t.call("linear", "t", "{}").await.result,
            Err(McpCallError::Transport(_))
        ));
    }

    #[tokio::test]
    async fn unknown_server_errors() {
        let t = static_transport();
        assert!(matches!(
            t.call("ghost", "t", "{}").await.result,
            Err(McpCallError::Transport(_))
        ));
    }
}
