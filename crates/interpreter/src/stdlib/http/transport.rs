//! Transport layer for `submilli:http` — the embedder-facing [`HttpClient`] /
//! [`AuthProxy`] traits, the default `reqwest` client with SSRF policy, and the
//! bounded download/decompression plumbing. No Wasm ABI here; the package's
//! host fns live in [`super`].

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;

#[derive(Clone, Debug)]
pub struct HttpRequest {
    /// Upper-cased.
    pub method: String,
    /// Absolute URL, query string included.
    pub url: String,
    /// Names as the guest supplied them; lookups downstream are case-insensitive.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout_ms: u64,
    /// `HttpError::TooLarge` if exceeded.
    pub max_response_size: u64,
    /// `http.download`-only: opt-in transparent decompression via `Content-Encoding` or URL suffix.
    pub decompress: bool,
}

/// 4xx/5xx are not errors at this layer — only transport failures become [`HttpError`].
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    /// Names lowercased by [`ReqwestHttpClient`].
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub final_url: String,
}

/// Body was streamed to the caller's writer; only metadata survives.
#[derive(Clone, Debug)]
pub struct DownloadMeta {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub final_url: String,
    pub bytes_written: u64,
}

#[derive(Debug)]
pub enum HttpError {
    Network(String),
    Timeout,
    /// Message includes the cap and suggests `http.download`.
    TooLarge {
        limit: u64,
    },
    /// Surfaces as a guest `TypeError` (a malformed request, not a transport failure).
    UnsupportedMethod(String),
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Network(msg) => write!(f, "network error: {msg}"),
            HttpError::Timeout => write!(f, "request timed out"),
            HttpError::TooLarge { limit } => write!(
                f,
                "response too large (limit: {limit} bytes); consider http.download",
            ),
            HttpError::UnsupportedMethod(method) => {
                write!(f, "unsupported HTTP method: {method}")
            }
            HttpError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for HttpError {}

/// Embedder-supplied HTTP transport. 4xx/5xx are not errors; only transport
/// failures return `Err(HttpError)`. `Send + Sync` for sharing across stores.
#[async_trait::async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError>;

    /// Git requires an exact destination: never follow redirects, even on the same host.
    /// Embedders must opt in to this contract before Git can use their transport.
    async fn send_without_redirects(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        Err(HttpError::Other(
            "transport does not support requests without redirects".into(),
        ))
    }

    /// No default impl: the obvious "buffer via `send` then `write_all`" fallback
    /// would silently break the bounded-memory guarantee `http.download` advertises.
    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError>;
}

/// Default `HttpClient` — async `reqwest`. Holds one `Client` (connection pool)
/// built with the SSRF policy resolver. The `cookies` feature is intentionally
/// never enabled, so the client carries **no** cross-request state; the server
/// builds one per session (see `submilli-server`) for tenant isolation.
pub struct ReqwestHttpClient {
    client: reqwest::Client,
    no_redirect_client: reqwest::Client,
    /// Also kept here (not just in the DNS resolver) so a **literal-IP** URL —
    /// which reqwest connects to without ever calling the resolver — is still
    /// checked. Without this, `http://127.0.0.1` would bypass the SSRF guard.
    policy: Arc<crate::stdlib::http::policy::NetworkPolicy>,
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new(Arc::new(
            crate::stdlib::http::policy::NetworkPolicy::allow_all(),
        ))
    }
}

impl ReqwestHttpClient {
    pub fn new(policy: Arc<crate::stdlib::http::policy::NetworkPolicy>) -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::default())
            .dns_resolver(std::sync::Arc::new(
                crate::stdlib::http::policy::PolicyResolver::new(Arc::clone(&policy)),
            ))
            .build()
            .expect("reqwest client builds with static config");
        let no_redirect_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(crate::stdlib::http::policy::PolicyResolver::new(
                Arc::clone(&policy),
            )))
            .build()
            .expect("reqwest client builds with static config");
        Self {
            client,
            no_redirect_client,
            policy,
        }
    }

    /// Block a literal-IP host the policy forbids. Hostnames go through the DNS
    /// resolver (which filters resolved IPs); literal IPs never hit it.
    fn check_literal_ip(&self, url: &str) -> Result<(), HttpError> {
        let ip = match url::Url::parse(url)
            .ok()
            .and_then(|u| u.host().map(|h| h.to_owned()))
        {
            Some(url::Host::Ipv4(v4)) => std::net::IpAddr::V4(v4),
            Some(url::Host::Ipv6(v6)) => std::net::IpAddr::V6(v6),
            _ => return Ok(()),
        };
        if self.policy.permits(ip) {
            Ok(())
        } else {
            Err(HttpError::Network(format!(
                "blocked by network policy: {ip} is private/loopback IP space; \
                 allow-list it on the server with --allow-ip / --allow-localhost / --allow-private"
            )))
        }
    }

    /// Shared request setup: method, URL, per-call timeout, headers.
    fn request(&self, req: &HttpRequest) -> Result<reqwest::RequestBuilder, HttpError> {
        self.request_with_client(req, &self.client)
    }

    fn request_with_client(
        &self,
        req: &HttpRequest,
        client: &reqwest::Client,
    ) -> Result<reqwest::RequestBuilder, HttpError> {
        self.check_literal_ip(&req.url)?;
        let method = reqwest::Method::from_bytes(req.method.to_ascii_uppercase().as_bytes())
            .map_err(|_| HttpError::UnsupportedMethod(req.method.clone()))?;
        let mut rb = client
            .request(method, &req.url)
            .timeout(Duration::from_millis(req.timeout_ms));
        for (name, value) in &req.headers {
            rb = rb.header(name.as_str(), value.as_str());
        }
        Ok(rb)
    }
}

#[async_trait::async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.send_with_client(req, &self.client).await
    }

    async fn send_without_redirects(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.send_with_client(req, &self.no_redirect_client).await
    }

    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        // GET-only in v1; trap here so a future "download via POST" doesn't
        // silently break the bounded-memory guarantee.
        if req.method != "GET" {
            return Err(HttpError::Other(format!(
                "http.download requires GET; got {}",
                req.method,
            )));
        }
        let resp = self.request(req)?.send().await.map_err(map_reqwest_error)?;

        let status = resp.status().as_u16();
        let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
        let final_url = resp.url().to_string();
        let headers = collect_headers(resp.headers());

        // Content-Encoding header takes precedence; URL suffix is the fallback.
        let kind = detect_decompression(&headers, &req.url, req.decompress);
        let limit = req.max_response_size;
        // Buffer the wire-capped body (≤ limit) then decode to the writer via the
        // shared sync path. TODO(Stage 2): stream straight to an async VFS sink
        // once the fs handles are `tokio::fs` (avoids buffering large downloads).
        let mut wire: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            if wire.len() as u64 + chunk.len() as u64 > limit {
                return Err(HttpError::TooLarge { limit });
            }
            wire.extend_from_slice(&chunk);
        }
        let bytes_written = stream_to_writer(std::io::Cursor::new(wire), writer, kind, limit)?;

        Ok(DownloadMeta {
            status,
            status_text,
            headers,
            final_url,
            bytes_written,
        })
    }
}

impl ReqwestHttpClient {
    async fn send_with_client(
        &self,
        req: &HttpRequest,
        client: &reqwest::Client,
    ) -> Result<HttpResponse, HttpError> {
        let mut rb = self.request_with_client(req, client)?;
        if !req.body.is_empty() {
            rb = rb.body(req.body.clone());
        }
        let resp = rb.send().await.map_err(map_reqwest_error)?;

        let status = resp.status().as_u16();
        let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
        let final_url = resp.url().to_string();
        let headers = collect_headers(resp.headers());

        // Bound host memory: stop reading once the wire body exceeds the cap.
        let limit = req.max_response_size;
        let mut body_bytes: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            if body_bytes.len() as u64 + chunk.len() as u64 > limit {
                return Err(HttpError::TooLarge { limit });
            }
            body_bytes.extend_from_slice(&chunk);
        }

        Ok(HttpResponse {
            status,
            status_text,
            headers,
            body: body_bytes,
            final_url,
        })
    }
}

/// Lowercase header names so `Headers#get("content-type")` matches any casing.
fn collect_headers(map: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    let mut headers = Vec::with_capacity(map.len());
    for (name, value) in map {
        if let Ok(v) = value.to_str() {
            headers.push((name.as_str().to_ascii_lowercase(), v.to_string()));
        }
    }
    headers
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decompression {
    None,
    Gzip,
    Zstd,
}

/// Public so embedder [`HttpClient::download`] impls can apply the right decoder
/// before counting bytes against `max_response_size`.
pub fn detect_decompression(
    headers: &[(String, String)],
    url: &str,
    decompress: bool,
) -> Decompression {
    if !decompress {
        return Decompression::None;
    }
    let ce = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
        .map_or("", |(_, v)| v.as_str());
    if ce.eq_ignore_ascii_case("gzip") || ce.eq_ignore_ascii_case("x-gzip") {
        return Decompression::Gzip;
    }
    if ce.eq_ignore_ascii_case("zstd") {
        return Decompression::Zstd;
    }
    // Header absent or `identity` — fall back to URL suffix.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".gz") {
        return Decompression::Gzip;
    }
    if lower.ends_with(".zst") {
        return Decompression::Zstd;
    }
    Decompression::None
}

/// Bounded chunked copy (8 KiB stack chunk) with optional gzip/zstd decoding.
/// Public so embedder [`HttpClient::download`] impls can reuse it.
pub fn stream_to_writer(
    reader: impl std::io::Read + 'static,
    writer: &mut dyn std::io::Write,
    kind: Decompression,
    limit: u64,
) -> Result<u64, HttpError> {
    use std::io::Read;
    let mut src: Box<dyn Read> = match kind {
        Decompression::None => Box::new(reader),
        Decompression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        Decompression::Zstd => Box::new(
            zstd::stream::read::Decoder::new(reader)
                .map_err(|e| HttpError::Network(format!("zstd: {e}")))?,
        ),
    };
    let mut total: u64 = 0;
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let n = match src.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(map_body_read_error(e, limit)),
        };
        if total.saturating_add(n as u64) > limit {
            return Err(HttpError::TooLarge { limit });
        }
        writer
            .write_all(&chunk[..n])
            .map_err(|e| HttpError::Network(format!("io: {e}")))?;
        total += n as u64;
    }
    Ok(total)
}

/// Read errors on the in-memory decode source map to `Network`; the wire-size
/// cap is enforced before `stream_to_writer`, so a `TooLarge` here would come
/// only from the decompressed-output guard inside `stream_to_writer` itself.
fn map_body_read_error(e: std::io::Error, _limit: u64) -> HttpError {
    HttpError::Network(format!("io: {e}"))
}

/// Bounded outcome class for an [`crate::runtime::metrics::HttpMetric`]: the transport failure mode.
pub(super) fn http_failure_outcome(err: &HttpError) -> &'static str {
    match err {
        HttpError::Timeout => "timeout",
        HttpError::TooLarge { .. } => "too_large",
        HttpError::Network(_) | HttpError::UnsupportedMethod(_) | HttpError::Other(_) => "error",
    }
}

/// reqwest reports timeout via `is_timeout()`; everything else is a network error.
fn map_reqwest_error(err: reqwest::Error) -> HttpError {
    if err.is_timeout() {
        return HttpError::Timeout;
    }
    HttpError::Network(err.to_string())
}

pub fn default_http_client() -> Arc<dyn HttpClient> {
    Arc::new(ReqwestHttpClient::default())
}

/// Pipeline: `host fn → auth_proxy.transform(req, caller) → http_client.send(req)`.
/// Consume-and-return so middlewares compose: `m2.transform(m1.transform(req, c)?, c)?`.
/// `caller` is the package that made the call (`"main"` for the user's script);
/// blueprint auth-proxy injection applies only to `main`.
#[async_trait::async_trait]
pub trait AuthProxy: Send + Sync {
    async fn transform(
        &self,
        req: HttpRequest,
        caller: &str,
    ) -> Result<HttpRequest, AuthProxyError>;
}

#[derive(Debug)]
pub enum AuthProxyError {
    /// Referenced a secret not declared in the blueprint's `secrets:` block.
    UndeclaredSecret(String),
    /// Secret declared but source returned no value.
    MissingSecret(String),
    Other(String),
}

impl std::fmt::Display for AuthProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthProxyError::UndeclaredSecret(name) => {
                write!(f, "undeclared secret '{name}'")
            }
            AuthProxyError::MissingSecret(name) => {
                write!(f, "missing secret '{name}'")
            }
            AuthProxyError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AuthProxyError {}

pub struct NoopAuthProxy;

#[async_trait::async_trait]
impl AuthProxy for NoopAuthProxy {
    async fn transform(
        &self,
        req: HttpRequest,
        _caller: &str,
    ) -> Result<HttpRequest, AuthProxyError> {
        Ok(req)
    }
}

pub fn default_auth_proxy() -> Arc<dyn AuthProxy> {
    Arc::new(NoopAuthProxy)
}
