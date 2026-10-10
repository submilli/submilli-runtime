//! Embedder hook for runtime observability. The interpreter times host-side
//! operations (currently HTTP transport) and hands each one to a [`MetricsSink`]
//! the embedder installs on [`StoreData`](super::StoreData). The default sink
//! discards everything, so the pure-interpreter path stays dependency-free —
//! the binary crates (CLI, server) are the only place a real metrics backend
//! (Sentry) is wired in.

/// One completed HTTP host operation, handed to [`MetricsSink::http_operation`]
/// after the transport returns (success or failure).
#[derive(Clone, Debug)]
pub struct HttpMetric {
    /// The gated capability that ran: `http.get`, `http.post`, `http.download`, …
    pub capability: String,
    /// Destination host from the request URL (e.g. `api.example.com`); empty when
    /// the URL doesn't parse.
    pub host: String,
    /// Wall-clock of the network transport — `send` plus capped body read for a
    /// request, or stream-plus-fsync for a download. Excludes arg marshalling.
    pub duration_ms: u64,
    /// HTTP status, or `0` if the request failed before any response arrived.
    pub status: u16,
    /// Bytes received: response-body length, or bytes written for a download.
    pub bytes: u64,
    /// Bounded outcome class: `ok`, `error`, `timeout`, or `too_large`.
    pub outcome: &'static str,
}

/// Embedder sink for runtime metrics. Every method has a no-op default so an
/// embedder only overrides what it reports on.
pub trait MetricsSink: Send + Sync {
    /// An HTTP host operation finished. Called once per `http_request` /
    /// `http_download` host call, on both the success and failure paths.
    fn http_operation(&self, _metric: HttpMetric) {}
}

/// Discards every metric. The [`StoreData`](super::StoreData) default.
pub struct NoopMetricsSink;

impl MetricsSink for NoopMetricsSink {}
