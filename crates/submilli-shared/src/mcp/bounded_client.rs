//! Bounded rmcp HTTP adapter. Protocol handling derives from rmcp 1.7.0's
//! reqwest adapter (Apache-2.0, see RMCP-LICENSE); modified to bound bodies
//! and events before parsing and to yield between events.

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use futures::{StreamExt, stream::BoxStream};
use http::{HeaderName, HeaderValue, header::WWW_AUTHENTICATE};
use interpreter::runtime::McpCallError;
use interpreter::runtime::mcp::MCP_MAX_RESPONSE_BYTES;
use reqwest::header::ACCEPT;
use sse_stream::{Sse, SseStream};
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

use rmcp::{
    model::{ClientJsonRpcMessage, JsonRpcMessage, ServerJsonRpcMessage},
    transport::{
        common::http_header::{
            EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_SESSION_ID, JSON_MIME_TYPE,
        },
        streamable_http_client::*,
    },
};

#[derive(Clone)]
pub(super) struct BoundedClient {
    client: reqwest::Client,
    pub budget: Arc<ResponseBudget>,
    request_timeout: std::time::Duration,
}

impl BoundedClient {
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            budget: Arc::new(ResponseBudget::default()),
            request_timeout: std::time::Duration::from_secs(60),
        }
    }
}

// Higher states win: an internal allocation failure must never be downgraded.
const RESPONSE_OK: u8 = 0;
const RESPONSE_LIMIT: u8 = 1;
const RESPONSE_INTERNAL: u8 = 2;

pub(super) struct ResponseBudget {
    limit: usize,
    failure: AtomicU8,
    received: AtomicU64,
    changed: tokio::sync::Notify,
    active: AtomicUsize,
    idle: tokio::sync::Notify,
}

impl Default for ResponseBudget {
    fn default() -> Self {
        Self {
            limit: MCP_MAX_RESPONSE_BYTES,
            failure: AtomicU8::new(RESPONSE_OK),
            received: AtomicU64::new(0),
            changed: tokio::sync::Notify::new(),
            active: AtomicUsize::new(0),
            idle: tokio::sync::Notify::new(),
        }
    }
}

/// Holds accounting open until an adapter request or detached SSE stream drops.
pub(super) struct ResponseActivity(Arc<ResponseBudget>);

impl Drop for ResponseActivity {
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

impl ResponseBudget {
    pub(super) fn activity(self: &Arc<Self>) -> ResponseActivity {
        self.active.fetch_add(1, Ordering::AcqRel);
        ResponseActivity(self.clone())
    }

    pub async fn wait_idle(&self) {
        loop {
            let notified = self.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }

    fn fail(&self) -> std::io::Error {
        self.latch(RESPONSE_LIMIT);
        std::io::Error::new(std::io::ErrorKind::FileTooLarge, "MCP response limit")
    }

    #[cfg(test)]
    pub(super) fn inject_allocation_failure(&self) {
        let _ = self.allocation_failed();
    }

    fn allocation_failed(&self) -> std::io::Error {
        self.latch(RESPONSE_INTERNAL);
        std::io::Error::from(std::io::ErrorKind::OutOfMemory)
    }

    fn latch(&self, kind: u8) {
        // An internal failure must not be downgraded by another stream.
        self.failure.fetch_max(kind, Ordering::AcqRel);
        self.changed.notify_one();
    }

    pub(super) fn record(&self, bytes: usize) {
        let _ = self
            .received
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |total| {
                Some(total.saturating_add(bytes as u64))
            });
    }

    pub fn take_received(&self) -> u64 {
        self.received.swap(0, Ordering::AcqRel)
    }

    pub fn error(&self) -> Option<McpCallError> {
        match self.failure.load(Ordering::Acquire) {
            RESPONSE_OK => None,
            RESPONSE_LIMIT => Some(McpCallError::ResponseTooLarge),
            _ => Some(McpCallError::Internal {
                message: "could not allocate MCP response buffer",
            }),
        }
    }

    fn check(&self) -> Result<(), StreamableHttpError<reqwest::Error>> {
        if self.error().is_some() {
            return Err(std::io::Error::other("MCP response stream stopped").into());
        }
        Ok(())
    }

    pub async fn wait(&self) -> McpCallError {
        loop {
            if let Some(error) = self.error() {
                return error;
            }
            self.changed.notified().await;
        }
    }
}

async fn bounded_body(
    response: reqwest::Response,
    budget: &ResponseBudget,
) -> Result<Vec<u8>, StreamableHttpError<reqwest::Error>> {
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        budget.record(chunk.len());
        if chunk.len() > budget.limit.saturating_sub(body.len()) {
            return Err(budget.fail().into());
        }
        body.try_reserve(chunk.len())
            .map_err(|_| budget.allocation_failed())?;
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Account raw framing bytes, including unfinished lines, before the SSE parser.
#[derive(Default)]
struct EventSize {
    bytes: usize,
    line_has_data: bool,
    pending_cr: bool,
}

impl EventSize {
    fn accept(&mut self, bytes: &[u8], limit: usize) -> bool {
        for &byte in bytes {
            if self.pending_cr && byte != b'\n' {
                self.finish_line();
            }
            self.bytes = self.bytes.saturating_add(1);
            if self.bytes > limit {
                return false;
            }
            if byte == b'\n' {
                self.finish_line();
            } else if byte == b'\r' {
                self.pending_cr = true;
            } else {
                self.line_has_data = true;
            }
        }
        true
    }

    fn finish_line(&mut self) {
        if !self.line_has_data {
            self.bytes = 0;
        }
        self.line_has_data = false;
        self.pending_cr = false;
    }
}

fn bounded_events(
    response: reqwest::Response,
    budget: Arc<ResponseBudget>,
) -> BoxStream<'static, Result<Sse, SseError>> {
    let activity = budget.activity();
    let chunks = response
        .bytes_stream()
        .scan(EventSize::default(), move |size, chunk| {
            let result = chunk.map_err(std::io::Error::other).and_then(|chunk| {
                budget.record(chunk.len());
                if budget.error().is_some() || !size.accept(&chunk, budget.limit) {
                    Err(budget.fail())
                } else {
                    Ok(chunk)
                }
            });
            futures::future::ready(Some(result))
        })
        .then(|chunk| async move {
            yield_once().await;
            chunk
        });
    SseStream::from_byte_stream(chunks)
        .then(|event| async move {
            // rmcp skips control/empty events recursively within poll_next. Yield
            // between events so that dependency recursion cannot accumulate.
            yield_once().await;
            event
        })
        .map(move |event| {
            let _ = &activity;
            event
        })
        .boxed()
}

// Both parsers can recurse while their upstream remains ready. One pending
// poll between chunks/events bounds those dependency call stacks.
async fn yield_once() {
    let mut yielded = false;
    futures::future::poll_fn(|cx| {
        if yielded {
            return std::task::Poll::Ready(());
        }
        yielded = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    })
    .await;
}

/// Applies custom headers to a request builder, rejecting reserved headers.
fn apply_custom_headers(
    mut builder: reqwest::RequestBuilder,
    custom_headers: HashMap<HeaderName, HeaderValue>,
) -> Result<reqwest::RequestBuilder, StreamableHttpError<reqwest::Error>> {
    for (name, value) in custom_headers {
        validate_custom_header(&name).map_err(StreamableHttpError::ReservedHeaderConflict)?;
        builder = builder.header(name, value);
    }
    Ok(builder)
}

/// Attempts to parse `body` as a JSON-RPC error message.
/// Returns `None` if the body is not parseable or is not a `JsonRpcMessage::Error`.
fn parse_json_rpc_error(body: &str) -> Option<ServerJsonRpcMessage> {
    match serde_json::from_str::<ServerJsonRpcMessage>(body) {
        Ok(message @ JsonRpcMessage::Error(_)) => Some(message),
        _ => None,
    }
}

impl StreamableHttpClient for BoundedClient {
    type Error = reqwest::Error;

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let _activity = self.budget.activity();
        self.budget.check()?;
        let mut request_builder = self
            .client
            .get(uri.as_ref())
            .header(ACCEPT, [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "))
            .header(HEADER_SESSION_ID, session_id.as_ref());
        if let Some(last_event_id) = last_event_id {
            request_builder = request_builder.header(HEADER_LAST_EVENT_ID, last_event_id);
        }
        if let Some(auth_header) = auth_token {
            request_builder = request_builder.bearer_auth(auth_header);
        }
        request_builder = apply_custom_headers(request_builder, custom_headers)?;
        let response = request_builder.send().await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        let response = response.error_for_status()?;
        match response.headers().get(reqwest::header::CONTENT_TYPE) {
            Some(ct) => {
                if !ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes())
                    && !ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes())
                {
                    return Err(StreamableHttpError::UnexpectedContentType(Some(
                        String::from_utf8_lossy(ct.as_bytes()).to_string(),
                    )));
                }
            }
            None => {
                return Err(StreamableHttpError::UnexpectedContentType(None));
            }
        }
        let event_stream = bounded_events(response, self.budget.clone());
        Ok(event_stream)
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let mut request_builder = self.client.delete(uri.as_ref());
        if let Some(auth_header) = auth_token {
            request_builder = request_builder.bearer_auth(auth_header);
        }
        request_builder = request_builder.header(HEADER_SESSION_ID, session.as_ref());
        request_builder = apply_custom_headers(request_builder, custom_headers)?;
        let response = request_builder.send().await?;

        // if method no allowed
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            tracing::debug!("this server doesn't support deleting session");
            return Ok(());
        }
        let _response = response.error_for_status()?;
        Ok(())
    }

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let _activity = self.budget.activity();
        self.budget.check()?;
        let mut request = self
            .client
            .post(uri.as_ref())
            .timeout(self.request_timeout)
            .header(ACCEPT, [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "));
        if let Some(auth_header) = auth_token {
            request = request.bearer_auth(auth_header);
        }

        request = apply_custom_headers(request, custom_headers)?;
        let session_was_attached = session_id.is_some();
        if let Some(session_id) = session_id {
            request = request.header(HEADER_SESSION_ID, session_id.as_ref());
        }
        let response = request.json(&message).send().await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && let Some(header) = response.headers().get(WWW_AUTHENTICATE)
        {
            let header = header
                .to_str()
                .map_err(|_| {
                    StreamableHttpError::UnexpectedServerResponse(Cow::from(
                        "invalid www-authenticate header value",
                    ))
                })?
                .to_string();
            return Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                header,
            )));
        }
        if response.status() == reqwest::StatusCode::FORBIDDEN
            && let Some(header) = response.headers().get(WWW_AUTHENTICATE)
        {
            let header_str = header.to_str().map_err(|_| {
                StreamableHttpError::UnexpectedServerResponse(Cow::from(
                    "invalid www-authenticate header value",
                ))
            })?;
            let scope = extract_scope_from_header(header_str);
            return Err(StreamableHttpError::InsufficientScope(
                InsufficientScopeError::new(header_str.to_string(), scope),
            ));
        }
        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == reqwest::StatusCode::NOT_FOUND && session_was_attached {
            return Err(StreamableHttpError::SessionExpired);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string());
        let session_id = response
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        // Non-success responses may carry valid JSON-RPC error payloads that
        // should be surfaced as McpError rather than lost in TransportSend.
        if !status.is_success() {
            let bytes = bounded_body(response, &self.budget).await?;
            let body = String::from_utf8_lossy(&bytes);
            if content_type
                .as_deref()
                .is_some_and(|ct| ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()))
            {
                if let Some(message) = parse_json_rpc_error(&body) {
                    return Ok(StreamableHttpPostResponse::Json(message, session_id));
                }
                tracing::warn!("HTTP {status}: could not parse JSON body as a JSON-RPC error");
            }
            return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                format!("HTTP {status}: {body}"),
            )));
        }
        match content_type.as_deref() {
            Some(ct) if ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) => {
                let event_stream = bounded_events(response, self.budget.clone());
                Ok(StreamableHttpPostResponse::Sse(event_stream, session_id))
            }
            Some(ct) if ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {
                // Try to parse as a valid JSON-RPC message. If the body is
                // malformed (e.g. a 200 response to a notification that lacks
                // an `id` field), treat it as accepted rather than failing.
                let body = bounded_body(response, &self.budget).await?;
                match serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                    Ok(message) => Ok(StreamableHttpPostResponse::Json(message, session_id)),
                    Err(e) => {
                        tracing::warn!(
                            "could not parse JSON response as ServerJsonRpcMessage, treating as accepted: {e}"
                        );
                        Ok(StreamableHttpPostResponse::Accepted)
                    }
                }
            }
            _ => {
                // unexpected content type
                tracing::error!("unexpected content type: {:?}", content_type);
                Err(StreamableHttpError::UnexpectedContentType(content_type))
            }
        }
    }
}

fn validate_custom_header(name: &http::HeaderName) -> Result<(), String> {
    if ["accept", HEADER_SESSION_ID, HEADER_LAST_EVENT_ID]
        .iter()
        .any(|reserved| name.as_str().eq_ignore_ascii_case(reserved))
    {
        return Err(name.to_string());
    }
    Ok(())
}

/// Extracts the `scope=` parameter from a `WWW-Authenticate` header value.
/// Handles both quoted (`scope="files:read files:write"`) and unquoted (`scope=read:data`) forms.
fn extract_scope_from_header(header: &str) -> Option<String> {
    let header_lowercase = header.to_ascii_lowercase();
    let scope_key = "scope=";

    if let Some(pos) = header_lowercase.find(scope_key) {
        let start = pos + scope_key.len();
        let value_slice = &header[start..];

        if let Some(stripped) = value_slice.strip_prefix('"') {
            if let Some(end_quote) = stripped.find('"') {
                return Some(stripped[..end_quote].to_string());
            }
        } else {
            let end = value_slice
                .find(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .unwrap_or(value_slice.len());
            if end > 0 {
                return Some(value_slice[..end].to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allocation_failure_latches_fatal_classification_and_received_work() {
        let budget = ResponseBudget::default();
        budget.record(32);
        let error = budget.allocation_failed();
        assert_eq!(error.kind(), std::io::ErrorKind::OutOfMemory);
        let _ = budget.fail();
        assert!(matches!(budget.wait().await, McpCallError::Internal { .. }));
        assert_eq!(budget.take_received(), 32);
        assert!(budget.check().is_err());
    }

    #[test]
    fn yield_boundary_requires_a_pending_poll() {
        let mut future = Box::pin(yield_once());
        let waker = futures::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_ready());
    }

    #[test]
    fn event_limit_covers_unfinished_lines_and_resets_only_at_blank_lines() {
        let mut size = EventSize::default();
        assert!(size.accept(b"data: x\n", 16));
        assert!(!size.accept(b"data: long", 16));
        for newline in ["\n", "\r", "\r\n"] {
            let frame = format!("data:x{newline}{newline}");
            let mut size = EventSize::default();
            for byte in frame.bytes().cycle().take(frame.len() * 4) {
                assert!(size.accept(&[byte], frame.len()));
            }
        }
        let mut size = EventSize::default();
        assert!(size.accept(b"abcdefgh", 8));
        assert!(!size.accept(b"i", 8));
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[tokio::test]
    async fn response_limit_precedes_json_and_sse_parsing_and_stops_reconnects() {
        let server = httpmock::MockServer::start_async().await;
        for (status, content_type, body) in [
            (200, "application/json", "x".repeat(65)),
            (500, "application/json", "x".repeat(65)),
            (
                200,
                "text/event-stream",
                format!("data: {}", "x".repeat(65)),
            ),
        ] {
            let mock = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST);
                    then.status(status)
                        .header("content-type", content_type)
                        .body(body);
                })
                .await;
            let budget = Arc::new(ResponseBudget {
                limit: 64,
                ..ResponseBudget::default()
            });
            let client = BoundedClient {
                client: reqwest::Client::new(),
                budget: budget.clone(),
                request_timeout: std::time::Duration::from_secs(60),
            };
            let message = serde_json::from_value(
                serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"ping"}),
            )
            .unwrap();
            let response = client
                .post_message(
                    server.url("/mcp").into(),
                    message,
                    None,
                    None,
                    HashMap::new(),
                )
                .await;
            if let Ok(StreamableHttpPostResponse::Sse(mut events, _)) = response {
                assert!(events.next().await.unwrap().is_err());
            } else {
                assert!(response.is_err());
            }
            assert!(matches!(
                budget.error(),
                Some(McpCallError::ResponseTooLarge)
            ));
            assert!(budget.take_received() >= 65);
            assert!(
                client
                    .get_stream(
                        server.url("/mcp").into(),
                        "session".into(),
                        None,
                        None,
                        HashMap::new()
                    )
                    .await
                    .is_err()
            );
            mock.assert_hits_async(1).await;
            mock.delete_async().await;
        }
    }
}
