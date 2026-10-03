//! Transport layer for `submilli:http` — the embedder-facing [`HttpClient`] /
//! [`AuthProxy`] traits, the default `reqwest` client with SSRF policy, and the
//! bounded download/decompression plumbing. No Wasm ABI here; the package's
//! host fns live in [`super`].

use std::sync::{Arc, OnceLock};

use super::{HttpTransportPolicy, TransportPolicyError};
use std::time::{Duration, Instant};

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
    /// Custom transports must enforce this on the initial URL and every redirect.
    pub transport_policy: Option<Arc<HttpTransportPolicy>>,
    /// Capability rules for redirect hops, attached after the auth proxy runs.
    /// Custom transports must consult it before sending every redirect hop; the
    /// initial URL is already authorized.
    pub redirect_guard: Option<Arc<dyn RedirectGuard>>,
}

/// Authorizes one redirect hop before any byte of it is sent.
pub trait RedirectGuard: Send + Sync + std::fmt::Debug {
    fn authorize(&self, hop: &RedirectHop<'_>) -> Result<(), RedirectDenied>;
}

/// The request a redirect is about to send.
#[derive(Clone, Copy, Debug)]
pub struct RedirectHop<'a> {
    /// Upper-cased; `GET` after a redirect rewrote the original method.
    pub method: &'a str,
    pub url: &'a url::Url,
    /// A redirect on the way here changed the method to `GET` and dropped the
    /// body: 301/302 for POST, 303 for every method but GET and HEAD.
    pub method_rewritten: bool,
    pub body_len: u64,
}

/// A hop the guard refused. Transports return it unchanged as
/// [`HttpError::PermissionDenied`] so the guest sees the original denial.
#[derive(Debug)]
pub struct RedirectDenied(wasmtime::Error);

impl RedirectDenied {
    /// A policy denial, surfaced to the guest as `PermissionDeniedError`.
    pub fn new(
        caller: impl Into<String>,
        capability: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self(crate::runtime::host::permission_denied(
            caller, capability, reason,
        ))
    }

    pub(crate) fn from_error(error: wasmtime::Error) -> Self {
        Self(error)
    }

    pub(crate) fn into_error(self) -> wasmtime::Error {
        self.0
    }
}

impl std::fmt::Display for RedirectDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for RedirectDenied {}

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
    /// Host setup failure: must terminate execution, not enter a guest catch.
    Internal(String),
    Policy(TransportPolicyError),
    /// A redirect hop the capability rules deny; nothing was sent to it.
    PermissionDenied(RedirectDenied),
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
            HttpError::Internal(msg) => write!(f, "internal HTTP transport error: {msg}"),
            HttpError::Policy(error) => error.fmt(f),
            HttpError::PermissionDenied(denied) => denied.fmt(f),
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
/// Implementations must enforce `HttpRequest::transport_policy`, including
/// redirect destinations, and must call `HttpRequest::redirect_guard` before
/// sending each redirect hop, returning its denial as
/// [`HttpError::PermissionDenied`]; a transport that follows redirects without
/// the guard bypasses the blueprint's capability rules. Preserve
/// `HttpError::Internal` as a fatal host failure.
#[async_trait::async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError>;

    /// Sends `req` to exactly its URL, never following a redirect, even on
    /// the same host, and writes the body to `body` as it arrives; the
    /// returned response's `body` is empty. Git fetches through it, streaming
    /// packs to disk, so an implementation holds one chunk in memory however
    /// large `max_response_size` is.
    ///
    /// Embedders must opt in to this contract before Git can use their
    /// transport: there is no buffering fallback, for the reason `download`
    /// has none.
    async fn send_without_redirects_to(
        &self,
        _req: &HttpRequest,
        _body: &mut (dyn std::io::Write + Send),
    ) -> Result<HttpResponse, HttpError> {
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

/// Default `HttpClient` — async `reqwest`, built with the SSRF policy resolver.
/// Redirects are followed by [`ReqwestHttpClient::follow_redirects`] rather than
/// reqwest, so every hop is checked against the request's own policy and guard
/// and the pooled client holds no per-request authorization. The `cookies`
/// feature is intentionally never enabled, so the client carries **no**
/// cross-request state; the server builds one per session (see
/// `submilli-server`) for tenant isolation.
pub struct ReqwestHttpClient {
    client: OnceLock<Result<reqwest::Client, String>>,
    /// Also kept here (not just in the DNS resolver) so a **literal-IP** URL —
    /// which reqwest connects to without ever calling the resolver — is still
    /// checked. Without this, `http://127.0.0.1` would bypass the SSRF guard.
    policy: Arc<crate::stdlib::http::policy::NetworkPolicy>,
}

/// Matches reqwest's default limit, which the resource-limit docs publish.
const MAX_REDIRECTS: usize = 10;

/// Removed when a redirect leaves the current origin, as reqwest does.
const CROSS_ORIGIN_SENSITIVE_HEADERS: [&str; 5] = [
    "authorization",
    "cookie",
    "cookie2",
    "proxy-authorization",
    "www-authenticate",
];

/// Headers describing the body that a method-rewriting redirect drops.
const BODY_HEADERS: [&str; 4] = [
    "content-type",
    "content-length",
    "content-encoding",
    "transfer-encoding",
];

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new(Arc::new(
            crate::stdlib::http::policy::NetworkPolicy::allow_all(),
        ))
    }
}

impl ReqwestHttpClient {
    pub fn new(policy: Arc<crate::stdlib::http::policy::NetworkPolicy>) -> Self {
        Self {
            client: OnceLock::new(),
            policy,
        }
    }

    /// Like [`Self::new`], with a prebuilt client whose builder a test customized.
    #[cfg(test)]
    pub(super) fn with_client(
        policy: Arc<crate::stdlib::http::policy::NetworkPolicy>,
        customize: impl FnOnce(reqwest::ClientBuilder) -> reqwest::ClientBuilder,
    ) -> Self {
        let client = Self::new(policy);
        let built = customize(client.client_builder())
            .build()
            .map_err(|error| error.to_string());
        let _ = client.client.set(built);
        client
    }

    fn client_builder(&self) -> reqwest::ClientBuilder {
        self.policy
            .client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
    }

    fn client(&self) -> Result<&reqwest::Client, HttpError> {
        self.client
            .get_or_init(|| {
                self.client_builder()
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|error| HttpError::Internal(error.clone()))
    }

    /// Block a literal-IP host the policy forbids. Hostnames go through the DNS
    /// resolver (which filters resolved IPs); literal IPs never hit it.
    fn check_literal_ip(&self, url: &str) -> Result<(), HttpError> {
        self.policy
            .check_literal_host(url)
            .map_err(HttpError::Network)
    }

    /// Send `req` and every redirect it earns. Each hop passes the transport
    /// policy, the network policy and the request's guard before it is sent, so
    /// a denied destination never receives a request or its body.
    async fn follow_redirects(&self, req: &HttpRequest) -> Result<reqwest::Response, HttpError> {
        let mut hop = self.initial_hop(req)?;
        let initial = hop.url.clone();
        let deadline = Deadline::new(req.timeout_ms);
        let mut redirects = 0;
        loop {
            let resp = self.send_hop(&hop, &deadline).await?;
            let Some(next) = redirect_location(&resp, &hop.url) else {
                return Ok(resp);
            };
            if redirects >= MAX_REDIRECTS {
                return Err(HttpError::Network(format!(
                    "too many redirects (limit: {MAX_REDIRECTS})"
                )));
            }
            redirects += 1;
            let status = resp.status();
            // The intermediate body is never read; dropping it abandons the connection.
            drop(resp);
            hop.redirect_to(status, next);
            self.authorize_hop(req, &initial, &hop)?;
        }
    }

    /// The request's own destination, checked as every hop is before it is sent.
    fn initial_hop<'a>(&self, req: &'a HttpRequest) -> Result<Hop<'a>, HttpError> {
        let url = parse_url(&req.url)?;
        if let Some(policy) = &req.transport_policy {
            policy.check_destination(&url).map_err(HttpError::Policy)?;
        }
        self.check_literal_ip(&req.url)?;
        Ok(Hop {
            method: parse_method(&req.method)?,
            url,
            headers: req.headers.clone(),
            body: &req.body,
            method_rewritten: false,
        })
    }

    /// Checks for a redirect hop, in the order the initial request applies them,
    /// ending with the capability guard. Runs before the hop is sent.
    fn authorize_hop(
        &self,
        req: &HttpRequest,
        initial: &url::Url,
        hop: &Hop<'_>,
    ) -> Result<(), HttpError> {
        if let Some(policy) = &req.transport_policy {
            policy
                .check_redirect(initial, &hop.url)
                .map_err(HttpError::Policy)?;
        }
        if !matches!(hop.url.scheme(), "http" | "https") {
            return Err(HttpError::Network(
                "redirect to a URL that is not http or https".into(),
            ));
        }
        self.check_literal_ip(hop.url.as_str())?;
        let Some(guard) = &req.redirect_guard else {
            return Ok(());
        };
        guard
            .authorize(&RedirectHop {
                method: hop.method.as_str(),
                url: &hop.url,
                method_rewritten: hop.method_rewritten,
                body_len: hop.body.len() as u64,
            })
            .map_err(HttpError::PermissionDenied)
    }

    /// One request with no redirect handling, bounded by what remains of `deadline`.
    async fn send_hop(
        &self,
        hop: &Hop<'_>,
        deadline: &Deadline,
    ) -> Result<reqwest::Response, HttpError> {
        let mut rb = self
            .client()?
            .request(hop.method.clone(), hop.url.clone())
            .timeout(deadline.remaining()?);
        for (name, value) in &hop.headers {
            rb = rb.header(name.as_str(), value.as_str());
        }
        if !hop.body.is_empty() {
            rb = rb.body(hop.body.to_vec());
        }
        rb.send().await.map_err(map_reqwest_error)
    }
}

/// The request the next hop sends. The body is the original request's until a
/// redirect drops it.
struct Hop<'a> {
    method: reqwest::Method,
    /// Keeps the request's userinfo, which reqwest sends as Basic auth.
    url: url::Url,
    headers: Vec<(String, String)>,
    body: &'a [u8],
    /// A 301/302/303 on the way here changed the method to GET.
    method_rewritten: bool,
}

impl Hop<'_> {
    /// Turn this hop into the redirect to `next` after a `status` response,
    /// matching reqwest's redirect policy: 301/302 turn POST into a bodiless GET,
    /// 303 turns every method but HEAD into GET and drops the body, and 307/308
    /// keep both. Credentials, including URL userinfo, stay within the origin.
    fn redirect_to(&mut self, status: reqwest::StatusCode, mut next: url::Url) {
        let rewrites_method = match status.as_u16() {
            301 | 302 => self.method == reqwest::Method::POST,
            303 => ![reqwest::Method::GET, reqwest::Method::HEAD].contains(&self.method),
            _ => false,
        };
        if rewrites_method {
            self.method = reqwest::Method::GET;
            self.method_rewritten = true;
        }
        if rewrites_method || status.as_u16() == 303 {
            self.body = &[];
            remove_headers(&mut self.headers, &BODY_HEADERS);
        }
        if self.url.origin() == next.origin() {
            keep_userinfo(&self.url, &mut next);
        } else {
            remove_headers(&mut self.headers, &CROSS_ORIGIN_SENSITIVE_HEADERS);
        }
        self.url = next;
    }
}

/// Carry `current`'s userinfo to a same-origin `next` that names none, like the
/// Basic auth header reqwest derives from userinfo.
fn keep_userinfo(current: &url::Url, next: &mut url::Url) {
    if !next.username().is_empty() || next.password().is_some() {
        return;
    }
    // Both URLs are http(s) with a host, which always accept userinfo.
    let _ = next.set_username(current.username());
    let _ = next.set_password(current.password());
}

fn remove_headers(headers: &mut Vec<(String, String)>, names: &[&str]) {
    headers.retain(|(name, _)| {
        !names
            .iter()
            .any(|removed| name.eq_ignore_ascii_case(removed))
    });
}

/// The destination of a redirect response to a request for `current`, or `None`
/// when the response is final: not a redirect status, or a `Location` that is
/// missing or unusable (reqwest returns such a response unchanged, too).
fn redirect_location(resp: &reqwest::Response, current: &url::Url) -> Option<url::Url> {
    if !matches!(resp.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    let location = resp.headers().get(reqwest::header::LOCATION)?;
    current
        .join(std::str::from_utf8(location.as_bytes()).ok()?)
        .ok()
}

fn parse_url(url: &str) -> Result<url::Url, HttpError> {
    url::Url::parse(url).map_err(|_| HttpError::Network("invalid HTTP URL".into()))
}

fn parse_method(method: &str) -> Result<reqwest::Method, HttpError> {
    reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
        .map_err(|_| HttpError::UnsupportedMethod(method.to_string()))
}

/// One timeout shared by every hop of a request, so the request timeout covers
/// the whole redirect chain.
struct Deadline {
    /// `None` when the timeout is too large to represent as an instant.
    at: Option<Instant>,
    timeout: Duration,
}

impl Deadline {
    fn new(timeout_ms: u64) -> Self {
        let timeout = Duration::from_millis(timeout_ms);
        Self {
            at: Instant::now().checked_add(timeout),
            timeout,
        }
    }

    fn remaining(&self) -> Result<Duration, HttpError> {
        let Some(at) = self.at else {
            return Ok(self.timeout);
        };
        let remaining = at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HttpError::Timeout);
        }
        Ok(remaining)
    }
}

#[async_trait::async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let resp = self.follow_redirects(req).await?;
        read_response(resp, req.max_response_size).await
    }

    async fn send_without_redirects_to(
        &self,
        req: &HttpRequest,
        body: &mut (dyn std::io::Write + Send),
    ) -> Result<HttpResponse, HttpError> {
        let hop = self.initial_hop(req)?;
        let resp = self.send_hop(&hop, &Deadline::new(req.timeout_ms)).await?;
        let status = resp.status().as_u16();
        let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
        let final_url = resp.url().to_string();
        let headers = collect_headers(resp.headers());
        let limit = req.max_response_size;
        let mut received: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            received = received.saturating_add(chunk.len() as u64);
            if received > limit {
                return Err(HttpError::TooLarge { limit });
            }
            body.write_all(&chunk)
                .map_err(|error| HttpError::Other(error.to_string()))?;
        }
        Ok(HttpResponse {
            status,
            status_text,
            headers,
            body: Vec::new(),
            final_url,
        })
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
        let resp = self.follow_redirects(req).await?;

        let status = resp.status().as_u16();
        let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
        let final_url = resp.url().to_string();
        let headers = collect_headers(resp.headers());

        // Content-Encoding header takes precedence; URL suffix is the fallback.
        let kind = detect_decompression(&headers, &req.url, req.decompress);
        let limit = req.max_response_size;
        // Each chunk goes to the file as it arrives, so host memory holds one
        // chunk however large `maxBytes` is. The limit applies to the wire bytes
        // and, separately, to the decoded bytes written.
        let mut sink = DecodeSink::new(kind, LimitedWriter::new(writer, limit))?;
        let mut wire: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            wire = wire.saturating_add(chunk.len() as u64);
            if wire > limit {
                return Err(HttpError::TooLarge { limit });
            }
            sink.write_all(&chunk)?;
        }
        let bytes_written = sink.finish()?;

        Ok(DownloadMeta {
            status,
            status_text,
            headers,
            final_url,
            bytes_written,
        })
    }
}

/// Buffer a response body, stopping once the wire body exceeds `limit` so host
/// memory stays bounded.
async fn read_response(resp: reqwest::Response, limit: u64) -> Result<HttpResponse, HttpError> {
    let status = resp.status().as_u16();
    let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
    let final_url = resp.url().to_string();
    let headers = collect_headers(resp.headers());

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
/// Public so embedder [`HttpClient::download`] impls can reuse it: `reader` is the
/// response body as it arrives, decoded into `writer` exactly as the built-in client
/// decodes a download, with the same limits on the wire and decoded bytes.
pub fn stream_to_writer(
    mut reader: impl std::io::Read,
    writer: &mut dyn std::io::Write,
    kind: Decompression,
    limit: u64,
) -> Result<u64, HttpError> {
    let mut sink = DecodeSink::new(kind, LimitedWriter::new(writer, limit))?;
    let mut wire: u64 = 0;
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| HttpError::Network(format!("io: {e}")))?;
        if n == 0 {
            break;
        }
        wire = wire.saturating_add(n as u64);
        if wire > limit {
            return Err(HttpError::TooLarge { limit });
        }
        sink.write_all(&chunk[..n])?;
    }
    sink.finish()
}

/// A writer that refuses bytes past `limit`, remembering that it did so, so a
/// decoder's error can be told apart from a download that is too large.
struct LimitedWriter<W> {
    inner: W,
    written: u64,
    limit: u64,
    exceeded: bool,
}

impl<W: std::io::Write> LimitedWriter<W> {
    fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            exceeded: false,
        }
    }
}

impl<W: std::io::Write> std::io::Write for LimitedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written.saturating_add(buf.len() as u64) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("download limit exceeded"));
        }
        let n = self.inner.write(buf)?;
        self.written = self.written.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// A download's destination: the file itself, or a decoder writing into it.
/// The decoders take the body a chunk at a time, and a truncated gzip or zstd
/// stream fails at `finish`.
enum DecodeSink<W: std::io::Write> {
    Plain(LimitedWriter<W>),
    Gzip(flate2::write::MultiGzDecoder<LimitedWriter<W>>),
    Zstd(zstd::stream::zio::Writer<LimitedWriter<W>, zstd::stream::raw::Decoder<'static>>),
}

impl<W: std::io::Write> DecodeSink<W> {
    fn new(kind: Decompression, out: LimitedWriter<W>) -> Result<Self, HttpError> {
        Ok(match kind {
            Decompression::None => Self::Plain(out),
            Decompression::Gzip => Self::Gzip(flate2::write::MultiGzDecoder::new(out)),
            Decompression::Zstd => {
                let decoder = zstd::stream::raw::Decoder::new()
                    .map_err(|e| HttpError::Network(format!("zstd: {e}")))?;
                Self::Zstd(zstd::stream::zio::Writer::new(out, decoder))
            }
        })
    }

    fn write_all(&mut self, chunk: &[u8]) -> Result<(), HttpError> {
        use std::io::Write;
        let result = match self {
            Self::Plain(out) => out.write_all(chunk),
            Self::Gzip(decoder) => decoder.write_all(chunk),
            Self::Zstd(decoder) => decoder.write_all(chunk),
        };
        result.map_err(|e| self.error(e))
    }

    /// Flush what the decoder still holds and return the bytes written.
    fn finish(mut self) -> Result<u64, HttpError> {
        use std::io::Write;
        let result = match &mut self {
            Self::Plain(out) => out.flush(),
            Self::Gzip(decoder) => decoder.try_finish(),
            Self::Zstd(decoder) => decoder.finish(),
        };
        result.map_err(|e| self.error(e))?;
        Ok(self.out().written)
    }

    fn out(&self) -> &LimitedWriter<W> {
        match self {
            Self::Plain(out) => out,
            Self::Gzip(decoder) => decoder.get_ref(),
            Self::Zstd(decoder) => decoder.writer(),
        }
    }

    fn error(&self, e: std::io::Error) -> HttpError {
        let out = self.out();
        if out.exceeded {
            HttpError::TooLarge { limit: out.limit }
        } else {
            HttpError::Network(format!("io: {e}"))
        }
    }
}

/// Bounded outcome class for an [`crate::runtime::metrics::HttpMetric`]: the transport failure mode.
pub(super) fn http_failure_outcome(err: &HttpError) -> &'static str {
    match err {
        HttpError::Timeout => "timeout",
        HttpError::TooLarge { .. } => "too_large",
        HttpError::Network(_)
        | HttpError::Internal(_)
        | HttpError::Policy(_)
        | HttpError::PermissionDenied(_)
        | HttpError::UnsupportedMethod(_)
        | HttpError::Other(_) => "error",
    }
}

/// reqwest reports timeout via `is_timeout()`; everything else is a network error.
///
/// reqwest's own message stops at "error sending request"; the reason lives at
/// the bottom of its source chain (a refused connection, or the policy
/// resolver's "blocked by network policy"). The guest and the operator both act
/// on that reason, so it is appended when it says more than the top-level message.
fn map_reqwest_error(err: reqwest::Error) -> HttpError {
    if err.is_timeout() {
        return HttpError::Timeout;
    }
    HttpError::Network(describe_error_chain(&err.without_url()))
}

pub fn describe_error_chain(err: &dyn std::error::Error) -> String {
    let top = err.to_string();
    let mut deepest: Option<String> = None;
    let mut current = err.source();
    while let Some(cause) = current {
        deepest = Some(cause.to_string());
        current = cause.source();
    }
    match deepest {
        Some(reason) if !top.contains(&reason) => format!("{top}: {reason}"),
        _ => top,
    }
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

#[cfg(test)]
mod tests {
    use super::describe_error_chain;

    #[derive(Debug)]
    struct Layer {
        message: &'static str,
        source: Option<Box<Layer>>,
    }

    impl std::fmt::Display for Layer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.message)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|layer| layer as &(dyn std::error::Error + 'static))
        }
    }

    #[test]
    fn appends_the_root_cause_when_the_top_message_omits_it() {
        let err = Layer {
            message: "error sending request for url (http://x/)",
            source: Some(Box::new(Layer {
                message: "client error (Connect)",
                source: Some(Box::new(Layer {
                    message: "blocked by network policy: x resolves only to private/loopback IP space",
                    source: None,
                })),
            })),
        };
        assert_eq!(
            describe_error_chain(&err),
            "error sending request for url (http://x/): blocked by network policy: x resolves only to private/loopback IP space"
        );
    }

    #[test]
    fn leaves_a_message_that_already_carries_its_cause_alone() {
        let err = Layer {
            message: "timeout: deadline elapsed",
            source: Some(Box::new(Layer {
                message: "deadline elapsed",
                source: None,
            })),
        };
        assert_eq!(describe_error_chain(&err), "timeout: deadline elapsed");
        let bare = Layer {
            message: "plain",
            source: None,
        };
        assert_eq!(describe_error_chain(&bare), "plain");
    }
}

#[cfg(test)]
#[path = "transport_tls_tests.rs"]
mod tls_tests;

#[cfg(test)]
mod decode_sink_tests {
    use std::io::Write as _;

    use super::{DecodeSink, Decompression, HttpError, LimitedWriter};

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// Feed `wire` in small chunks, as a network stream would.
    fn decode(kind: Decompression, wire: &[u8], limit: u64) -> (Result<u64, HttpError>, Vec<u8>) {
        let mut out: Vec<u8> = Vec::new();
        let result = (|| {
            let mut sink = DecodeSink::new(kind, LimitedWriter::new(&mut out, limit))?;
            for chunk in wire.chunks(7) {
                sink.write_all(chunk)?;
            }
            sink.finish()
        })();
        (result, out)
    }

    #[test]
    fn plain_bytes_stop_at_the_limit() {
        let (ok, out) = decode(Decompression::None, b"hello", 5);
        assert_eq!(ok.unwrap(), 5);
        assert_eq!(out, b"hello");
        let (over, _) = decode(Decompression::None, b"hello!", 5);
        assert!(matches!(over, Err(HttpError::TooLarge { limit: 5 })));
    }

    #[test]
    fn gzip_decodes_across_chunks_and_rejects_truncation() {
        let wire = gzip(b"hello gzipped download");
        let (ok, out) = decode(Decompression::Gzip, &wire, 1024);
        assert_eq!(ok.unwrap(), 22);
        assert_eq!(out, b"hello gzipped download");

        let (truncated, _) = decode(Decompression::Gzip, &wire[..wire.len() - 4], 1024);
        assert!(
            matches!(truncated, Err(HttpError::Network(_))),
            "{truncated:?}"
        );
    }

    #[test]
    fn a_small_gzip_body_that_inflates_past_the_limit_is_too_large() {
        let wire = gzip(&vec![b'x'; 100_000]);
        assert!(wire.len() < 1_000);
        let (result, out) = decode(Decompression::Gzip, &wire, 1_000);
        assert!(
            matches!(result, Err(HttpError::TooLarge { limit: 1_000 })),
            "{result:?}"
        );
        assert!(out.len() <= 1_000);
    }

    #[test]
    fn zstd_decodes_and_rejects_an_incomplete_frame() {
        let wire = zstd::encode_all(&b"hello zstd download"[..], 1).unwrap();
        let (ok, out) = decode(Decompression::Zstd, &wire, 1024);
        assert_eq!(ok.unwrap(), 19);
        assert_eq!(out, b"hello zstd download");

        let (truncated, _) = decode(Decompression::Zstd, &wire[..wire.len() - 3], 1024);
        assert!(
            matches!(truncated, Err(HttpError::Network(_))),
            "{truncated:?}"
        );
    }
}

#[cfg(test)]
mod client_setup_tests {
    use super::*;

    #[test]
    fn invalid_client_configuration_is_retained_as_an_internal_failure() {
        let client = ReqwestHttpClient::with_client(
            Arc::new(crate::stdlib::http::policy::NetworkPolicy::allow_all()),
            |builder| builder.user_agent("\n"),
        );
        for _ in 0..2 {
            assert!(matches!(client.client(), Err(HttpError::Internal(_))));
        }
        let healthy = ReqwestHttpClient::default();
        assert!(healthy.client().is_ok());
        assert!(healthy.client().is_ok());
    }
}
