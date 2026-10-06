//! The outbound embedding provider: the runtime side of `embedding.embed`.
//!
//! Mirrors [`crate::llm`]. [`BlueprintEmbeddingProvider`] is an
//! [`interpreter::runtime::EmbeddingProvider`] bound to one blueprint's
//! `embedding:` block. It owns everything independent of a provider's wire
//! format: alias resolution, embedding-space identity, sub-batch splitting,
//! response validation, normalization and settlement. The wire format and HTTP
//! live behind [`EmbeddingDispatch`], which is also the test seam.
//!
//! **No input text and no provider response body leaves this module.** A
//! [`DispatchFailure`] carries a classification, never a body.

pub mod dispatch;
pub mod provider;
pub mod wire;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use interpreter::runtime::{EmbeddingMalformedReason, Purpose};
use submilli_blueprint::EmbeddingProviderType;

pub use dispatch::{HttpEmbeddingDispatch, HttpEmbeddingDispatchError};
pub use provider::{BlueprintEmbeddingProvider, DEFAULT_MAX_CONCURRENCY};

/// One outbound request, fully resolved: everything an adapter needs to build
/// the wire request for one sub-batch. `texts` already fits the adapter's
/// input-count and token caps.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddingRequest<'a> {
    /// The blueprint's provider key, for credential lookup.
    pub provider: &'a str,
    pub provider_type: EmbeddingProviderType,
    /// The provider's own model identifier.
    pub model: &'a str,
    pub base_url: Option<&'a str>,
    /// The provider's `api_key` value as declared (a `${secrets.X}` reference),
    /// unresolved. `None` only for a Hugging Face endpoint declared without one.
    pub api_key: Option<&'a str>,
    /// The alias's declared vector length.
    pub dimensions: u64,
    /// Whether to send `dimensions` as the provider's parameter. When false it
    /// is only checked against the response.
    pub send_dimensions: bool,
    pub purpose: Purpose,
    /// Hugging Face prompt names; `None` elsewhere.
    pub query_prompt_name: Option<&'a str>,
    pub document_prompt_name: Option<&'a str>,
    pub texts: &'a [String],
}

/// One returned embedding row, before validation.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchRow {
    /// The provider's `index` field, when it sends one. Either every row has
    /// one or none does.
    pub index: Option<usize>,
    pub values: Vec<f32>,
}

/// A successful response. The wire layer returns rows only for a 2-D shape; a
/// token-level (3-D) response is reported as
/// [`DispatchFailure::Malformed`] with [`EmbeddingMalformedReason::InvalidBody`].
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchResponse {
    pub rows: Vec<DispatchRow>,
    /// Provider-reported input tokens for the whole request. `None`, or zero,
    /// means unreported.
    pub usage: Option<u64>,
}

/// Why the provider refused a request with a 4xx status. All of these are free:
/// nothing is charged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// An input exceeded the provider's length limit. `index` is local to the
    /// request's `texts`, when the provider names it.
    InputTooLong {
        index: Option<usize>,
    },
    Unauthorized,
    /// HTTP 429. `retry_after` is for an adapter that retries; the core does
    /// not surface it.
    RateLimited {
        retry_after: Option<Duration>,
    },
    /// Any other 4xx.
    Other,
}

/// A request that reached the provider (or its network path) and then failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentFailure {
    /// 5xx.
    ProviderUnavailable,
    Transport,
    Timeout,
}

/// Why a request never left the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSentReason {
    /// The provider's `api_key` could not be resolved.
    CredentialUnresolved,
    /// The network policy refused the destination (a literal address or a
    /// resolved one).
    Blocked,
    /// The request could not be built or no connection could be made.
    Unreachable,
}

/// How a dispatch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchFailure {
    /// Nothing was sent. The request had already been counted, so its estimate
    /// is released.
    NotSent(NotSentReason),
    /// The provider rejected the request with a 4xx status.
    Rejected(Rejection),
    /// Sent, then a 5xx, transport failure, or timeout. `usage` is what the
    /// provider reported, if anything.
    Failed {
        kind: SentFailure,
        usage: Option<u64>,
    },
    /// Sent, and the response was unusable at the wire level (not JSON, wrong
    /// shape, 3-D token-level output).
    Malformed {
        reason: EmbeddingMalformedReason,
        usage: Option<u64>,
    },
}

/// One outbound embedding request. The seam the HTTP adapters sit behind, so the
/// provider core can be tested against hand-rolled fakes with zero live calls.
pub trait EmbeddingDispatch: Send + Sync {
    fn dispatch<'a>(
        &'a self,
        request: EmbeddingRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>>;

    /// Check the provider's credential before any request is counted or sent,
    /// so a missing key is one `Unauthorized` rather than a charged request.
    /// Only [`DispatchFailure::NotSent`] (credential unresolved) and
    /// [`Rejection::Unauthorized`] are meaningful here; the default accepts.
    fn preflight<'a>(
        &'a self,
        _provider: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), DispatchFailure>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
