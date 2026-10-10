//! The embedder-supplied seam for `submilli:embedding` — batches of text embedded
//! by a remote provider from inside a Submilli program.
//!
//! Mirrors [`crate::runtime::llm`]: the HTTP, key resolution and retry live in
//! the embedder behind [`EmbeddingProvider`], and token accounting runs on the
//! shared [`TokenLedger`].
//!
//! **No input text appears here.** Not in an error `Display`, not in a log line,
//! not in a settlement. Errors carry the alias, numbers, an input *index*, and a
//! fixed kebab-case classification string; provider response bodies are dropped
//! at this boundary.
//!
//! # Settlement protocol
//!
//! A call is split by the provider into sub-batches, each sent as one outbound
//! request. The stdlib reserves the whole estimate up front
//! ([`EmbeddingTokenBudget::reserve`]) and hands the provider the budget. For
//! every sub-batch the provider:
//!
//! 1. calls [`EmbeddingTokenBudget::mark_sent`] with that sub-batch's estimate
//!    *immediately before* sending. This counts the request against the per-run
//!    cap (refusing past it) and moves the estimate from "reserved" to "held",
//!    so a future dropped mid-flight leaves the sub-batch held while unsent
//!    sub-batches are returned when the budget drops;
//! 2. records a [`SubBatchSettlement`] when the response (or failure) arrives.
//!
//! Settlements travel back on [`EmbeddingBatch::settlements`] or inside the
//! error, and the stdlib applies them with [`EmbeddingTokenBudget::settle`],
//! which also releases whatever was reserved but never sent. The outcome table
//! the provider applies to each *sent* sub-batch:
//!
//! | Sub-batch outcome | `reported` | `indeterminate` |
//! |---|---|---|
//! | Succeeded with nonzero usage | usage | 0 |
//! | Succeeded without usage, or usage 0 | 0 | estimate |
//! | Never sent (credential unresolved, blocked by network policy, connect failure) | 0 | 0 (released) |
//! | Rejected by provider with 4xx (incl. 429) | 0 | 0 (released) |
//! | Sent, then 5xx / transport / timeout / failed validation | usage if reported | else estimate |
//! | Sent, then the future is dropped (cancelled) | 0 | estimate |
//!
//! Sub-batches never sent have no settlement; they are released by `settle`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

pub use super::token_ledger::SharedTokenBudget;
use super::token_ledger::{LedgerLimits, LedgerRefusal, TokenLedger};

/// The single internal host module every `submilli:embedding` call dispatches
/// through.
pub const EMBEDDING_MODULE_NAME: &str = "submilli:embedding";

/// Tokens one execution may spend across every embedding call it makes.
pub const DEFAULT_MAX_EXECUTION_EMBEDDING_TOKENS: u64 = 2_000_000;
/// Tokens one execution may hold as indeterminate reserve.
pub const DEFAULT_MAX_HELD_EMBEDDING_TOKENS: u64 = 200_000;
/// Outbound provider requests one execution may send.
pub const DEFAULT_MAX_EMBEDDING_REQUESTS: u64 = 1_000;
/// Texts in one call.
pub const DEFAULT_MAX_TEXTS_PER_CALL: u64 = 128;
/// UTF-8 bytes across one call's texts.
pub const DEFAULT_MAX_BYTES_PER_CALL: u64 = 2 * 1024 * 1024;

/// The ceilings an embedding call is checked against.
///
/// Concurrency per call is deliberately *not* here: it is how a provider fans
/// out its sub-batches, so it lives in the provider's configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingLimits {
    pub per_execution_tokens: u64,
    pub max_held_tokens: u64,
    pub max_requests: u64,
    pub max_texts_per_call: u64,
    pub max_bytes_per_call: u64,
}

impl Default for EmbeddingLimits {
    fn default() -> Self {
        Self {
            per_execution_tokens: DEFAULT_MAX_EXECUTION_EMBEDDING_TOKENS,
            max_held_tokens: DEFAULT_MAX_HELD_EMBEDDING_TOKENS,
            max_requests: DEFAULT_MAX_EMBEDDING_REQUESTS,
            max_texts_per_call: DEFAULT_MAX_TEXTS_PER_CALL,
            max_bytes_per_call: DEFAULT_MAX_BYTES_PER_CALL,
        }
    }
}

/// The token estimate for one input: `ceil(utf8_bytes / 3)`.
pub fn estimate_embedding_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(3)
}

/// What an embedding is for. Some providers embed queries and documents
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Query,
    Document,
}

impl Purpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Document => "document",
        }
    }
}

/// Which ceiling refused a call. Carries numbers, never inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingLimitKind {
    PerExecutionTokens {
        requested: u64,
        limit: u64,
    },
    AllExecutionsTokens {
        requested: u64,
        limit: u64,
    },
    /// Held (unreported-usage) reserve, including the call's own worst case.
    HeldTokens {
        held: u64,
        limit: u64,
    },
    /// Outbound provider requests in this execution.
    Requests {
        limit: u64,
    },
}

impl std::fmt::Display for EmbeddingLimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerExecutionTokens { requested, limit } => write!(
                f,
                "execution embedding token budget: {requested} tokens exceeds the {limit} this \
                 execution may spend — embed fewer or shorter texts, or split the work across \
                 executions; the operator raises it with `--max-execution-embedding-tokens`"
            ),
            Self::AllExecutionsTokens { requested, limit } => write!(
                f,
                "server embedding token budget: {requested} tokens exceeds the {limit} allowed \
                 across all live executions — this execution's own spend is not what is in the \
                 way; the operator raises the budget with `--max-embedding-tokens`"
            ),
            Self::HeldTokens { held, limit } => write!(
                f,
                "indeterminate embedding spend ceiling: {held} tokens are already held for requests \
                 whose usage the provider did not report, past the {limit} a single execution \
                 may hold — start a new execution to embed more"
            ),
            Self::Requests { limit } => write!(
                f,
                "embedding request limit: this execution already sent {limit} provider requests, \
                 the most it may send — embed more texts per call instead of calling more often; \
                 the operator raises it with `--max-execution-embedding-requests`"
            ),
        }
    }
}

/// Which input bound a call ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBoundKind {
    TextCount,
    TotalBytes,
}

impl std::fmt::Display for EmbeddingBoundKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextCount => write!(f, "texts in one call"),
            Self::TotalBytes => write!(f, "bytes in one call"),
        }
    }
}

/// Why a sent request failed, as a closed vocabulary (the wire form is
/// [`Self::as_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingFailureReason {
    RateLimited,
    RequestRejected,
    ProviderUnavailable,
    Transport,
    Timeout,
    /// The provider's `base_url` is refused by the network policy. Nothing
    /// was sent.
    BlockedByNetworkPolicy,
}

impl EmbeddingFailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RateLimited => "rate-limited",
            Self::RequestRejected => "request-rejected",
            Self::ProviderUnavailable => "provider-unavailable",
            Self::Transport => "transport",
            Self::Timeout => "timeout",
            Self::BlockedByNetworkPolicy => "blocked-by-network-policy",
        }
    }

    pub const ALL: [EmbeddingFailureReason; 6] = [
        Self::RateLimited,
        Self::RequestRejected,
        Self::ProviderUnavailable,
        Self::Transport,
        Self::Timeout,
        Self::BlockedByNetworkPolicy,
    ];
}

impl std::fmt::Display for EmbeddingFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a provider response was unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingMalformedReason {
    CountMismatch,
    DimensionMismatch,
    NonFiniteValue,
    InvalidBody,
}

impl EmbeddingMalformedReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CountMismatch => "count-mismatch",
            Self::DimensionMismatch => "dimension-mismatch",
            Self::NonFiniteValue => "non-finite-value",
            Self::InvalidBody => "invalid-body",
        }
    }
}

impl std::fmt::Display for EmbeddingMalformedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one *sent* sub-batch cost. `estimate` is what was passed to
/// [`EmbeddingTokenBudget::mark_sent`]; see the module table for the other two.
/// The remainder (`estimate - reported - indeterminate`) is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SubBatchSettlement {
    pub estimate: u64,
    pub reported: u64,
    pub indeterminate: u64,
}

/// Why an embedding call failed. Variants after the pre-send group carry the
/// settlements of every sub-batch that was sent before the failure.
///
/// None carries input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    NotConfigured {
        alias: String,
    },
    UnknownModel {
        alias: String,
        available: Vec<String>,
    },
    /// An input exceeds the alias's `max_input_bytes`, or the provider rejected
    /// one as too long. `index` names the input when known. `limit` is the byte
    /// limit for the pre-send check and `None` for a provider-side rejection,
    /// which carries the settlements of sub-batches already sent.
    InputTooLong {
        alias: String,
        index: Option<usize>,
        limit: Option<u64>,
        settlements: Vec<SubBatchSettlement>,
    },
    BoundsExceeded {
        alias: String,
        kind: EmbeddingBoundKind,
        actual: u64,
        limit: u64,
    },
    /// A ceiling refused the call. Pre-send, `settlements` is empty; the
    /// request cap can also trip between sub-batches.
    BudgetExceeded {
        alias: String,
        limit_kind: EmbeddingLimitKind,
        settlements: Vec<SubBatchSettlement>,
    },
    Unauthorized {
        alias: String,
        settlements: Vec<SubBatchSettlement>,
    },
    Provider {
        alias: String,
        reason: EmbeddingFailureReason,
        settlements: Vec<SubBatchSettlement>,
    },
    Malformed {
        alias: String,
        reason: EmbeddingMalformedReason,
        settlements: Vec<SubBatchSettlement>,
    },
    /// The provider's own bookkeeping failed (a closed pool, a missing slot).
    /// Never guest-catchable: the stdlib settles the budget and ends the run.
    Internal {
        alias: String,
        settlements: Vec<SubBatchSettlement>,
    },
}

impl EmbeddingError {
    pub fn alias(&self) -> &str {
        match self {
            Self::NotConfigured { alias }
            | Self::UnknownModel { alias, .. }
            | Self::InputTooLong { alias, .. }
            | Self::BoundsExceeded { alias, .. }
            | Self::BudgetExceeded { alias, .. }
            | Self::Unauthorized { alias, .. }
            | Self::Provider { alias, .. }
            | Self::Malformed { alias, .. }
            | Self::Internal { alias, .. } => alias,
        }
    }

    /// Per-sub-batch settlements carried by a post-send failure; empty for
    /// pre-send variants.
    pub fn settlements(&self) -> &[SubBatchSettlement] {
        match self {
            Self::BudgetExceeded { settlements, .. }
            | Self::Unauthorized { settlements, .. }
            | Self::Provider { settlements, .. }
            | Self::Malformed { settlements, .. }
            | Self::Internal { settlements, .. }
            | Self::InputTooLong { settlements, .. } => settlements,
            Self::NotConfigured { .. }
            | Self::UnknownModel { .. }
            | Self::BoundsExceeded { .. } => &[],
        }
    }

    /// Token-budget refusals become catchable `QuotaExceededError`s at the
    /// stdlib boundary.
    pub fn is_budget_exceeded(&self) -> bool {
        matches!(self, Self::BudgetExceeded { .. })
    }

    /// A provider-side invariant failure that must end the run.
    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal { .. })
    }

    /// Attach settlements to a failure raised by [`EmbeddingTokenBudget::mark_sent`]
    /// or built before the provider had them. A no-op on pre-send variants.
    pub fn with_settlements(mut self, collected: Vec<SubBatchSettlement>) -> Self {
        match &mut self {
            Self::BudgetExceeded { settlements, .. }
            | Self::Unauthorized { settlements, .. }
            | Self::Provider { settlements, .. }
            | Self::Malformed { settlements, .. }
            | Self::Internal { settlements, .. }
            | Self::InputTooLong { settlements, .. } => *settlements = collected,
            Self::NotConfigured { .. }
            | Self::UnknownModel { .. }
            | Self::BoundsExceeded { .. } => {}
        }
        self
    }
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured { alias } => write!(
                f,
                "embedding.embed(\"{alias}\"): no embedding provider is configured for this \
                 runtime — the operator wires one before embeddings can run"
            ),
            Self::UnknownModel { alias, available } => {
                write!(
                    f,
                    "embedding.embed(\"{alias}\"): the provider does not serve that alias — "
                )?;
                if available.is_empty() {
                    write!(
                        f,
                        "it serves none, so declare an embedding model in the blueprint"
                    )
                } else {
                    write!(
                        f,
                        "call embedding.models() and use one of: {}",
                        available.join(", ")
                    )
                }
            }
            Self::InputTooLong {
                alias,
                index,
                limit,
                ..
            } => {
                write!(f, "embedding.embed(\"{alias}\"): ")?;
                match (limit, index) {
                    (Some(limit), Some(index)) => write!(
                        f,
                        "text {index} exceeds the {limit} bytes this model accepts — shorten or \
                         split it"
                    ),
                    (Some(limit), None) => write!(
                        f,
                        "a text exceeds the {limit} bytes this model accepts — shorten or split it"
                    ),
                    (None, Some(index)) => write!(
                        f,
                        "the provider rejected text {index} as too long for this model — shorten \
                         or split it"
                    ),
                    (None, None) => write!(
                        f,
                        "the provider rejected a text as too long for this model — shorten or \
                         split it"
                    ),
                }
            }
            Self::BoundsExceeded {
                alias,
                kind,
                actual,
                limit,
            } => write!(
                f,
                "embedding.embed(\"{alias}\"): input bound exceeded: {actual} {kind} exceeds the \
                 {limit} allowed — send fewer or shorter texts per call"
            ),
            Self::BudgetExceeded {
                alias, limit_kind, ..
            } => write!(f, "embedding.embed(\"{alias}\") exceeded the {limit_kind}"),
            Self::Unauthorized { alias, .. } => write!(
                f,
                "embedding.embed(\"{alias}\"): the provider rejected the API credential, or none \
                 was available — the operator sets a valid key for this provider"
            ),
            Self::Provider { alias, reason, .. } => write!(
                f,
                "embedding.embed(\"{alias}\"): the provider request failed: {reason}"
            ),
            Self::Malformed { alias, reason, .. } => write!(
                f,
                "embedding.embed(\"{alias}\"): the provider returned an unusable response: \
                 {reason}"
            ),
            Self::Internal { alias, .. } => {
                write!(f, "embedding.embed(\"{alias}\"): internal host failure")
            }
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// Row access or construction outside an [`EmbeddingBatch`]'s shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingShapeError {
    /// `vectors.len()` is not `count * dimensions`, or the product overflows.
    LengthMismatch {
        len: usize,
        count: usize,
        dimensions: usize,
    },
    RowOutOfRange {
        index: usize,
        count: usize,
    },
}

impl std::fmt::Display for EmbeddingShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthMismatch {
                len,
                count,
                dimensions,
            } => write!(
                f,
                "embedding batch holds {len} values but {count} rows of {dimensions} dimensions \
                 were declared"
            ),
            Self::RowOutOfRange { index, count } => {
                write!(f, "embedding row {index} is out of range for {count} rows")
            }
        }
    }
}

impl std::error::Error for EmbeddingShapeError {}

/// The result of one successful call: `count` vectors of `dimensions` floats,
/// row-major, labeled with the embedding-space `identity`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    vectors: Vec<f32>,
    count: usize,
    dimensions: usize,
    identity: String,
    model: String,
    input_tokens: Option<u64>,
    settlements: Vec<SubBatchSettlement>,
}

impl EmbeddingBatch {
    /// Validates `vectors.len() == count * dimensions` (checked multiply).
    pub fn new(
        vectors: Vec<f32>,
        count: usize,
        dimensions: usize,
        identity: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, EmbeddingShapeError> {
        let expected = count.checked_mul(dimensions);
        if expected != Some(vectors.len()) {
            return Err(EmbeddingShapeError::LengthMismatch {
                len: vectors.len(),
                count,
                dimensions,
            });
        }
        Ok(Self {
            vectors,
            count,
            dimensions,
            identity: identity.into(),
            model: model.into(),
            input_tokens: None,
            settlements: Vec::new(),
        })
    }

    /// Provider-reported total input tokens, when it reported any.
    pub fn with_input_tokens(mut self, input_tokens: Option<u64>) -> Self {
        self.input_tokens = input_tokens;
        self
    }

    /// The settlements of every sub-batch this call sent, for
    /// [`EmbeddingTokenBudget::settle`].
    pub fn with_settlements(mut self, settlements: Vec<SubBatchSettlement>) -> Self {
        self.settlements = settlements;
        self
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn input_tokens(&self) -> Option<u64> {
        self.input_tokens
    }

    pub fn settlements(&self) -> &[SubBatchSettlement] {
        &self.settlements
    }

    /// All values, row-major.
    pub fn values(&self) -> &[f32] {
        &self.vectors
    }

    /// Row `index`, or an error past `count`. Never panics.
    pub fn row(&self, index: usize) -> Result<&[f32], EmbeddingShapeError> {
        let out_of_range = EmbeddingShapeError::RowOutOfRange {
            index,
            count: self.count,
        };
        if index >= self.count {
            return Err(out_of_range);
        }
        let start = index.checked_mul(self.dimensions).ok_or(out_of_range)?;
        let end = start.checked_add(self.dimensions).ok_or(out_of_range)?;
        self.vectors.get(start..end).ok_or(out_of_range)
    }
}

/// An embedding model alias the provider serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModel {
    pub name: String,
    pub description: Option<String>,
    pub dimensions: u64,
    pub max_input_tokens: Option<u64>,
    pub max_input_bytes: u64,
    /// The embedding-space identity string vectors are labeled with.
    pub identity: String,
}

/// The embedder-provided embedding provider: resolves credentials, performs the
/// requests, and reports what each cost.
///
/// # Obligations on every implementor
///
/// 1. **Positional ordering.** The returned batch has one row per input text,
///    in input order, all of `dimensions` width and finite.
/// 2. **Bounded concurrency.** The guest is single-threaded; fan-out across
///    sub-batches lives here, bounded by the implementor.
/// 3. **Count before sending.** Call [`EmbeddingTokenBudget::mark_sent`] with
///    the sub-batch's estimate immediately before each outbound request, and
///    never send if it refuses. Retries are outbound requests too.
/// 4. **Settle honestly.** Record a [`SubBatchSettlement`] per sent sub-batch
///    per the module's outcome table; unknown spend is `indeterminate`, not
///    free. Return them on the batch or inside every post-send error
///    ([`EmbeddingError::with_settlements`]).
/// 5. **Drop safety.** The future may be dropped at any await. State the budget
///    holds must stay correct then: do `mark_sent` and the send with no await
///    between them that could skip the send but keep the hold (a hold without a
///    send is merely conservative, never unsafe).
/// 6. **No input text in errors**, and no provider response bodies.
pub trait EmbeddingProvider: Send + Sync {
    fn embed<'a>(
        &'a self,
        alias: &'a str,
        texts: &'a [String],
        purpose: Purpose,
        budget: &'a EmbeddingTokenBudget,
    ) -> Pin<Box<dyn Future<Output = Result<EmbeddingBatch, EmbeddingError>> + Send + 'a>>;

    /// The locally declared catalog, without network I/O.
    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<EmbeddingModel>, EmbeddingError>> + Send + 'a>>;

    /// Per-alias input byte limit, consulted before dispatch. `None` means the
    /// alias is not served.
    fn max_input_bytes(&self, alias: &str) -> Option<u64>;

    /// Token estimate for reserving `texts` against `alias`. The default is
    /// `ceil(utf8_bytes / 3)` per input; saturating.
    fn estimate_tokens(&self, _alias: &str, texts: &[String]) -> u64 {
        texts.iter().fold(0u64, |total, text| {
            total.saturating_add(estimate_embedding_tokens(text))
        })
    }
}

/// One execution's embedding budget: tokens against its own ceiling and the
/// server-wide one, plus the outbound request counter.
pub struct EmbeddingTokenBudget {
    limits: EmbeddingLimits,
    ledger: TokenLedger,
    requests: AtomicU64,
}

impl EmbeddingTokenBudget {
    pub fn new(limits: EmbeddingLimits, aggregate: SharedTokenBudget) -> Self {
        let ledger = TokenLedger::new(
            LedgerLimits {
                per_execution_tokens: limits.per_execution_tokens,
                max_held_tokens: limits.max_held_tokens,
            },
            aggregate,
        );
        Self {
            limits,
            ledger,
            requests: AtomicU64::new(0),
        }
    }

    /// A budget that never refuses, for runs with no operator-installed budget.
    pub fn unmetered() -> Self {
        Self::new(
            EmbeddingLimits {
                per_execution_tokens: u64::MAX,
                max_held_tokens: u64::MAX,
                max_requests: u64::MAX,
                ..EmbeddingLimits::default()
            },
            SharedTokenBudget::new(u64::MAX),
        )
    }

    pub fn limits(&self) -> EmbeddingLimits {
        self.limits
    }

    pub fn used(&self) -> u64 {
        self.ledger.used()
    }

    pub fn held(&self) -> u64 {
        self.ledger.held()
    }

    /// Outbound requests counted so far.
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// Reserve a call's whole estimate before dispatch. Like `submilli:llm`,
    /// refused once held reserve is already past its cap, so held spend per run
    /// is bounded by the cap plus one call's sent estimate.
    pub fn reserve(&self, alias: &str, tokens: u64) -> Result<(), EmbeddingError> {
        let refused = |limit_kind| EmbeddingError::BudgetExceeded {
            alias: alias.to_string(),
            limit_kind,
            settlements: Vec::new(),
        };
        self.ledger.reserve(tokens).map_err(|refusal| {
            refused(match refusal {
                LedgerRefusal::HeldReserve { held, limit } => {
                    EmbeddingLimitKind::HeldTokens { held, limit }
                }
                LedgerRefusal::PerExecution { requested, limit } => {
                    EmbeddingLimitKind::PerExecutionTokens { requested, limit }
                }
                LedgerRefusal::AllExecutions { requested, limit } => {
                    EmbeddingLimitKind::AllExecutionsTokens { requested, limit }
                }
            })
        })
    }

    /// Count one outbound request and move `estimate` (already reserved) to
    /// held. Call immediately before sending; on `Err`, do not send. The error
    /// is a [`EmbeddingError::BudgetExceeded`] with no settlements — attach the
    /// provider's with [`EmbeddingError::with_settlements`].
    pub fn mark_sent(&self, alias: &str, estimate: u64) -> Result<(), EmbeddingError> {
        let max = self.limits.max_requests;
        let counted = self
            .requests
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |sent| {
                (sent < max).then(|| sent.saturating_add(1))
            });
        if counted.is_err() {
            return Err(EmbeddingError::BudgetExceeded {
                alias: alias.to_string(),
                limit_kind: EmbeddingLimitKind::Requests { limit: max },
                settlements: Vec::new(),
            });
        }
        self.ledger.reconcile(estimate, 0, estimate);
        Ok(())
    }

    /// Apply a call's settlements and release whatever of `reserved` was never
    /// sent. Idempotence is not provided: call once per reservation.
    pub fn settle(&self, reserved: u64, settlements: &[SubBatchSettlement]) {
        let mut sent = 0u64;
        for settlement in settlements {
            self.ledger.settle_held(
                settlement.estimate,
                settlement.reported,
                settlement.indeterminate,
            );
            sent = sent.saturating_add(settlement.estimate);
        }
        let unsent = reserved.saturating_sub(sent);
        if unsent > 0 {
            self.ledger.reconcile(unsent, 0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "SECRET-SAMPLE-INPUT-TEXT";

    fn limits() -> EmbeddingLimits {
        EmbeddingLimits {
            per_execution_tokens: 1_000,
            max_held_tokens: 500,
            max_requests: 3,
            ..EmbeddingLimits::default()
        }
    }

    fn budget(aggregate: &SharedTokenBudget) -> EmbeddingTokenBudget {
        EmbeddingTokenBudget::new(limits(), aggregate.clone())
    }

    fn settle_one(estimate: u64, reported: u64, indeterminate: u64) -> SubBatchSettlement {
        SubBatchSettlement {
            estimate,
            reported,
            indeterminate,
        }
    }

    #[test]
    fn default_limits_match_the_plan() {
        let l = EmbeddingLimits::default();
        assert_eq!(l.per_execution_tokens, 2_000_000);
        assert_eq!(l.max_held_tokens, 200_000);
        assert_eq!(l.max_requests, 1_000);
        assert_eq!(l.max_texts_per_call, 128);
        assert_eq!(l.max_bytes_per_call, 2 * 1024 * 1024);
    }

    #[test]
    fn estimate_is_ceil_bytes_over_three() {
        assert_eq!(estimate_embedding_tokens(""), 0);
        assert_eq!(estimate_embedding_tokens("ab"), 1);
        assert_eq!(estimate_embedding_tokens("abcd"), 2);
        assert_eq!(estimate_embedding_tokens("é"), 1);
    }

    #[test]
    fn reserve_over_the_per_run_cap_charges_nothing() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = EmbeddingTokenBudget::new(
            EmbeddingLimits {
                max_held_tokens: 5_000,
                ..limits()
            },
            aggregate.clone(),
        );
        let err = b.reserve("m", 1_001).expect_err("over cap");
        assert!(matches!(
            err,
            EmbeddingError::BudgetExceeded {
                limit_kind: EmbeddingLimitKind::PerExecutionTokens {
                    requested: 1_001,
                    limit: 1_000
                },
                ..
            }
        ));
        assert!(err.to_string().contains("--max-execution-embedding-tokens"));
        assert_eq!((b.used(), aggregate.used()), (0, 0));
    }

    #[test]
    fn aggregate_refusal_names_the_server_flag() {
        let aggregate = SharedTokenBudget::new(10);
        let b = budget(&aggregate);
        let err = b.reserve("m", 11).expect_err("over aggregate");
        assert!(err.to_string().contains("--max-embedding-tokens"));
        assert_eq!((b.used(), aggregate.used()), (0, 0));
    }

    #[test]
    fn reported_usage_below_estimate_refunds_the_difference() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 300).expect("fits");
        b.mark_sent("m", 300).expect("sent");
        b.settle(300, &[settle_one(300, 120, 0)]);
        assert_eq!((b.used(), b.held(), aggregate.used()), (120, 0, 120));
    }

    #[test]
    fn reported_usage_above_the_estimate_is_charged() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 1).expect("fits");
        b.mark_sent("m", 1).expect("sent");
        b.settle(1, &[settle_one(1, 7, 0)]);
        assert_eq!((b.used(), b.held(), aggregate.used()), (7, 0, 7));
    }

    #[test]
    fn no_usage_keeps_the_estimate_held_past_the_run() {
        let aggregate = SharedTokenBudget::new(10_000);
        {
            let b = budget(&aggregate);
            b.reserve("m", 300).expect("fits");
            b.mark_sent("m", 300).expect("sent");
            b.settle(300, &[settle_one(300, 0, 300)]);
            assert_eq!((b.used(), b.held()), (300, 300));
        }
        assert_eq!(aggregate.used(), 300);
    }

    #[test]
    fn failed_call_charges_reported_and_indeterminate_and_releases_the_rest() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 400).expect("fits");
        b.mark_sent("m", 100).expect("first sent");
        b.mark_sent("m", 100).expect("second sent");
        // First succeeded with usage 40; second failed with no usage; the
        // remaining 200 were never sent.
        b.settle(400, &[settle_one(100, 40, 0), settle_one(100, 0, 100)]);
        assert_eq!((b.used(), b.held(), aggregate.used()), (140, 100, 140));
    }

    #[test]
    fn rejected_sub_batch_releases_everything() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 100).expect("fits");
        b.mark_sent("m", 100).expect("sent");
        b.settle(100, &[settle_one(100, 0, 0)]);
        assert_eq!((b.used(), b.held(), aggregate.used()), (0, 0, 0));
    }

    #[test]
    fn sent_then_dropped_stays_held_and_unsent_is_returned() {
        let aggregate = SharedTokenBudget::new(10_000);
        {
            let b = budget(&aggregate);
            b.reserve("m", 300).expect("fits");
            b.mark_sent("m", 100).expect("sent");
            // Future dropped here: no settle.
        }
        assert_eq!(aggregate.used(), 100);
    }

    #[test]
    fn a_call_is_refused_once_held_reserve_is_past_the_cap() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 600)
            .expect("one call may exceed the held cap");
        b.mark_sent("m", 600).expect("sent");
        b.settle(600, &[settle_one(600, 0, 600)]);
        let err = b.reserve("m", 1).expect_err("held is past the cap");
        assert!(matches!(
            err,
            EmbeddingError::BudgetExceeded {
                limit_kind: EmbeddingLimitKind::HeldTokens {
                    held: 600,
                    limit: 500
                },
                ..
            }
        ));
        assert!(err.to_string().contains("held"));
        assert_eq!(b.used(), 600);
    }

    #[test]
    fn held_reserve_at_the_cap_still_admits_a_call() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 500).expect("fits");
        b.mark_sent("m", 500).expect("sent");
        b.settle(500, &[settle_one(500, 0, 500)]);
        b.reserve("m", 100)
            .expect("held equals the cap, not past it");
    }

    #[test]
    fn request_past_the_cap_is_refused_before_sent() {
        let aggregate = SharedTokenBudget::new(10_000);
        let b = budget(&aggregate);
        b.reserve("m", 100).expect("fits");
        for _ in 0..3 {
            b.mark_sent("m", 1).expect("within cap");
        }
        let held_before = b.held();
        let err = b.mark_sent("m", 1).expect_err("fourth refused");
        assert!(matches!(
            err,
            EmbeddingError::BudgetExceeded {
                limit_kind: EmbeddingLimitKind::Requests { limit: 3 },
                ..
            }
        ));
        assert!(
            err.to_string()
                .contains("--max-execution-embedding-requests")
        );
        assert_eq!(b.requests(), 3);
        assert_eq!(b.held(), held_before, "a refused request holds nothing");
    }

    #[test]
    fn dropping_the_budget_returns_unspent_tokens() {
        let aggregate = SharedTokenBudget::new(10_000);
        {
            let b = budget(&aggregate);
            b.reserve("m", 250).expect("fits");
            assert_eq!(aggregate.used(), 250);
        }
        assert_eq!(aggregate.used(), 0);
    }

    #[test]
    fn unmetered_budget_never_refuses() {
        let b = EmbeddingTokenBudget::unmetered();
        b.reserve("m", 5_000_000).expect("fits");
        for _ in 0..2_000 {
            b.mark_sent("m", 0).expect("uncapped");
        }
    }

    #[test]
    fn batch_row_access_is_checked() {
        let batch = EmbeddingBatch::new(vec![1.0, 2.0, 3.0, 4.0], 2, 2, "id", "m").expect("shape");
        assert_eq!(batch.row(0), Ok(&[1.0, 2.0][..]));
        assert_eq!(batch.row(1), Ok(&[3.0, 4.0][..]));
        assert_eq!(
            batch.row(2),
            Err(EmbeddingShapeError::RowOutOfRange { index: 2, count: 2 })
        );
        assert!(batch.row(usize::MAX).is_err());
    }

    #[test]
    fn batch_construction_validates_length_with_checked_mul() {
        assert!(EmbeddingBatch::new(vec![1.0; 3], 2, 2, "id", "m").is_err());
        assert!(EmbeddingBatch::new(vec![], usize::MAX, 2, "id", "m").is_err());
        assert!(EmbeddingBatch::new(vec![], 0, 5, "id", "m").is_ok());
    }

    #[test]
    fn no_error_display_contains_input_text() {
        let settlements = vec![settle_one(1, 0, 1)];
        let errors = vec![
            EmbeddingError::NotConfigured { alias: "m".into() },
            EmbeddingError::UnknownModel {
                alias: "m".into(),
                available: vec!["a".into()],
            },
            EmbeddingError::InputTooLong {
                alias: "m".into(),
                index: Some(2),
                limit: Some(9),
                settlements: Vec::new(),
            },
            EmbeddingError::InputTooLong {
                alias: "m".into(),
                index: Some(2),
                limit: None,
                settlements: Vec::new(),
            },
            EmbeddingError::BoundsExceeded {
                alias: "m".into(),
                kind: EmbeddingBoundKind::TextCount,
                actual: 200,
                limit: 128,
            },
            EmbeddingError::BudgetExceeded {
                alias: "m".into(),
                limit_kind: EmbeddingLimitKind::Requests { limit: 1 },
                settlements: settlements.clone(),
            },
            EmbeddingError::Unauthorized {
                alias: "m".into(),
                settlements: settlements.clone(),
            },
            EmbeddingError::Provider {
                alias: "m".into(),
                reason: EmbeddingFailureReason::Transport,
                settlements: settlements.clone(),
            },
            EmbeddingError::Malformed {
                alias: "m".into(),
                reason: EmbeddingMalformedReason::CountMismatch,
                settlements: settlements.clone(),
            },
            EmbeddingError::Internal {
                alias: "m".into(),
                settlements,
            },
        ];
        for error in errors {
            let text = error.to_string();
            assert!(!text.contains(SAMPLE), "{text}");
            assert!(text.contains("\"m\""), "{text}");
        }
    }

    #[test]
    fn a_provider_side_length_rejection_does_not_claim_a_byte_limit() {
        let provider_side = EmbeddingError::InputTooLong {
            alias: "m".into(),
            index: Some(1),
            limit: None,
            settlements: Vec::new(),
        }
        .to_string();
        assert!(
            provider_side.contains("the provider rejected text 1 as too long"),
            "{provider_side}"
        );
        assert!(!provider_side.contains("bytes"), "{provider_side}");
        let pre_send = EmbeddingError::InputTooLong {
            alias: "m".into(),
            index: Some(1),
            limit: Some(9),
            settlements: Vec::new(),
        }
        .to_string();
        assert!(pre_send.contains("exceeds the 9 bytes"), "{pre_send}");
    }

    #[test]
    fn post_send_errors_carry_settlements() {
        let err = EmbeddingError::Internal {
            alias: "m".into(),
            settlements: Vec::new(),
        }
        .with_settlements(vec![settle_one(5, 0, 5)]);
        assert_eq!(err.settlements(), &[settle_one(5, 0, 5)]);
        let pre = EmbeddingError::NotConfigured { alias: "m".into() }
            .with_settlements(vec![settle_one(5, 0, 5)]);
        assert!(pre.settlements().is_empty());
    }

    #[test]
    fn failure_reason_spellings_are_stable_kebab_case() {
        let spelled: Vec<&str> = EmbeddingFailureReason::ALL
            .iter()
            .map(EmbeddingFailureReason::as_str)
            .collect();
        assert_eq!(
            spelled,
            [
                "rate-limited",
                "request-rejected",
                "provider-unavailable",
                "transport",
                "timeout",
                "blocked-by-network-policy"
            ]
        );
    }
}
