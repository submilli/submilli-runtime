//! The embedder-supplied seam for `submilli:llm` — gated model calls from inside
//! a Submilli program.
//!
//! `call`, `batch`, and `models()` all dispatch through the single
//! embedder-provided [`LlmProvider`]: the actual HTTP, key resolution, retry, and
//! provider-SDK dependencies live in the embedder (submilli-server), keeping this
//! crate free of them — the same split [`crate::runtime::mcp::McpTransport`] makes.
//!
//! **Two taxonomies, answering different questions.** [`LlmCallError`] is the
//! *dispatch-level* `Err`: the request never reached the model at all, or the
//! transport died before any element ran. [`FailureReason`] is *per-element*: the
//! request reached the model and that one element came back unusable or
//! incomplete. Neither is a subset of the other, and a per-element failure must
//! never be raised as an `Err` — a whole-batch `Err` would discard every element
//! outcome including successes that were already billed, and would leave
//! per-element budget reconciliation with nothing to reconcile against.
//!
//! **No prompt or completion text appears here.** Not in an error `Display`, not
//! in a log line, not in a permission-check context, and not in an
//! [`LlmFailure::message`]. The taxonomies carry the model name, the numbers, a
//! status code, and a fixed classification string — never the payload. Provider
//! response bodies are dropped at this boundary specifically: they carry
//! completion fragments, request echoes, and account identifiers, so unlike
//! [`McpCallError::Upstream`](crate::runtime::mcp::McpCallError::Upstream) no
//! variant here interpolates one.

use std::future::Future;
use std::pin::Pin;

pub use super::token_ledger::SharedTokenBudget;
use super::token_ledger::{LedgerLimits, LedgerRefusal, TokenLedger};

/// The single internal host module every `submilli:llm` call dispatches through.
pub const LLM_MODULE_NAME: &str = "submilli:llm";

/// Tokens one execution may spend across every model call it makes.
pub const DEFAULT_MAX_EXECUTION_TOKENS: u64 = 1_000_000;
/// Tokens every live execution may spend between them.
pub const DEFAULT_MAX_ALL_EXECUTIONS_TOKENS: u64 = 20_000_000;
/// Tokens one execution may hold as indeterminate (null-usage) reserve before
/// further dispatch is refused. See [`LlmLimitKind::IndeterminateSpend`].
pub const DEFAULT_MAX_HELD_TOKENS: u64 = 200_000;
/// Output tokens reserved per prompt when the model declares no `outputReserve`.
pub const DEFAULT_OUTPUT_CAP: u64 = 64_000;
/// Elements in one `batch` slice.
pub const DEFAULT_MAX_PROMPT_COUNT: u64 = 128;
/// UTF-8 bytes in a single prompt element.
pub const DEFAULT_MAX_PROMPT_BYTES: u64 = 256 * 1024;

/// The ceilings a dispatch is checked against.
///
/// Token ceilings and prompt bounds live in one struct deliberately: an operator
/// raising how much a program may spend almost always means to raise how much it
/// may send, and splitting them across two config surfaces makes that two edits
/// with a silent failure between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmLimits {
    /// Tokens one execution may spend in total.
    pub per_execution_tokens: u64,
    /// Tokens one execution may hold as indeterminate reserve before further
    /// dispatch is refused.
    pub max_held_tokens: u64,
    /// Output tokens reserved per prompt when the model declares no
    /// `outputReserve` of its own.
    pub default_output_cap: u64,
    /// Elements in one `batch` slice. Enforced at the stdlib boundary.
    pub max_prompt_count: u64,
    /// UTF-8 bytes in a single prompt element. Enforced at the stdlib boundary.
    pub max_prompt_bytes: u64,
}

impl Default for LlmLimits {
    fn default() -> Self {
        Self {
            per_execution_tokens: DEFAULT_MAX_EXECUTION_TOKENS,
            max_held_tokens: DEFAULT_MAX_HELD_TOKENS,
            default_output_cap: DEFAULT_OUTPUT_CAP,
            max_prompt_count: DEFAULT_MAX_PROMPT_COUNT,
            max_prompt_bytes: DEFAULT_MAX_PROMPT_BYTES,
        }
    }
}

/// Which token ceiling a refused dispatch ran into. Carries the numbers, never
/// the prompts — the same rule
/// [`SessionKvLimitKind`](crate::runtime::session_kv::SessionKvLimitKind)
/// follows for values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmLimitKind {
    /// This execution's own token ceiling.
    PerExecutionTokens { requested: u64, limit: u64 },
    /// The server-wide budget summed across every live execution. Distinct from
    /// [`Self::PerExecutionTokens`] because it wants the opposite response: the
    /// tokens in the way belong to other executions, so this execution making
    /// fewer or shorter calls need not help.
    AllExecutionsTokens { requested: u64, limit: u64 },
    /// Cumulative reserve held for elements whose usage the provider never
    /// reported. Token spend is monotonic — unlike session-KV bytes, a held
    /// estimate is never superseded by a real number — so without this bound a
    /// long-lived execution ratchets toward zero with no recovery. Distinct from
    /// a genuine overspend so an operator can tell a null-usage storm from real
    /// consumption.
    IndeterminateSpend { held: u64, limit: u64 },
}

impl std::fmt::Display for LlmLimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerExecutionTokens { requested, limit } => write!(
                f,
                "execution token budget: {requested} tokens exceeds the {limit} this execution \
                 may spend — use fewer or shorter calls, or split the work across executions"
            ),
            Self::AllExecutionsTokens { requested, limit } => write!(
                f,
                "server token budget: {requested} tokens exceeds the {limit} allowed across all \
                 live executions — this execution's own spend is not what is in the way, so \
                 using fewer or shorter calls need not help; the operator raises the budget \
                 with `--max-llm-tokens`"
            ),
            // No CLI rung names this ceiling: `max_held_tokens` is set by the
            // embedder, unlike the two above. Saying otherwise would send an
            // operator looking for a flag that does not exist, so the advice is
            // the action they can actually take.
            Self::IndeterminateSpend { held, limit } => write!(
                f,
                "indeterminate spend ceiling reached: {held} tokens are held for calls the \
                 provider never reported usage for, exceeding the {limit} allowed — this is \
                 unreported spend, not measured consumption; start a new execution, and if it \
                 recurs the provider is not reporting usage and the operator should raise the \
                 held-token ceiling in the server configuration"
            ),
        }
    }
}

/// Why a model *dispatch* failed — the request never reached the model, or the
/// transport died before any element ran.
///
/// Reserved for dispatch-level failure only. A model that answered and produced
/// an unusable or incomplete element reports that in [`LlmOutcome`]'s failure
/// arm, never here: returning `Err` for one bad element would throw away the
/// sibling elements that did succeed.
///
/// Every variant names the model it was dispatched at and the fix the caller can
/// act on. None carries prompt or completion text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmCallError {
    /// No [`LlmProvider`] is wired into the runtime, so nothing can dispatch.
    /// The pure-interpreter path leaves the provider `None`, and this is the
    /// catchable configuration error a script sees there.
    NotConfigured { model: String },
    /// The model name is not one the provider serves. `available` lists what it
    /// does serve, so the message can name the alternatives rather than only the
    /// rejection.
    UnknownModel {
        model: String,
        available: Vec<String>,
    },
    /// A token ceiling cannot cover this dispatch. Charged before any element
    /// ran, so nothing was billed.
    ///
    /// `limit_kind` carries which ceiling refused, because the three want
    /// different responses and a message that reads the same for all of them
    /// sends a caller to shrink work that was never what filled the budget.
    BudgetExceeded {
        model: String,
        limit_kind: LlmLimitKind,
    },
    /// The prompt slice is outside the bounds the host accepts — too many
    /// elements, or one element too large. Counted in elements and bytes, never
    /// quoted.
    PromptBoundsExceeded {
        model: String,
        limit_kind: PromptBoundKind,
        actual: u64,
        limit: u64,
    },
    /// The provider rejected the credential, or none was resolvable when the
    /// provider was constructed. One variant covers both because the fix is the
    /// same key — this is the `LoadAPIKeyError`-vs-`401` split collapsed at the
    /// point where the caller can act on it.
    Unauthorized { model: String },
    /// The request never completed, and died before any element ran. A transport
    /// failure *after* elements started belongs in [`FailureReason::Transport`]
    /// on the affected elements instead.
    ///
    /// `detail` is a **fixed classification string** — "connection reset",
    /// "dns lookup failed", "tls handshake failed". It is the one free-form field
    /// in this taxonomy, and the no-payload rule binds it: never a provider response body, and
    /// never a request echo. The precedent this trait otherwise mirrors does
    /// interpolate a body
    /// ([`McpCallError::Upstream`](crate::runtime::mcp::McpCallError::Upstream)
    /// renders `HTTP {status}: {body}`); this one deliberately does not, because a
    /// model provider's body carries completion fragments.
    Transport { model: String, detail: String },
}

/// Which prompt bound a dispatch ran into. Carries the numbers, never the
/// prompts — the same rule
/// [`SessionKvLimitKind`](crate::runtime::session_kv::SessionKvLimitKind) follows
/// for values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptBoundKind {
    /// Elements in one `call` / `batch` slice.
    PromptCount,
    /// UTF-8 bytes of a single prompt element.
    PromptBytes,
}

impl std::fmt::Display for PromptBoundKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PromptCount => write!(f, "prompts in one batch"),
            Self::PromptBytes => write!(f, "bytes in a single prompt"),
        }
    }
}

impl std::fmt::Display for LlmCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured { model } => write!(
                f,
                "llm.call(\"{model}\"): no model provider is configured for this runtime — the \
                 operator wires one before model calls can dispatch"
            ),
            Self::UnknownModel { model, available } => {
                write!(
                    f,
                    "llm.call(\"{model}\"): the provider does not serve that model — "
                )?;
                if available.is_empty() {
                    write!(
                        f,
                        "it serves none, so declare a model in the blueprint before calling"
                    )
                } else {
                    write!(
                        f,
                        "call llm.models() and use one of: {}",
                        available.join(", ")
                    )
                }
            }
            Self::BudgetExceeded { model, limit_kind } => {
                write!(f, "llm.call(\"{model}\") exceeded the {limit_kind}")
            }
            Self::PromptBoundsExceeded {
                model,
                limit_kind,
                actual,
                limit,
            } => write!(
                f,
                "llm.call(\"{model}\"): prompt bound exceeded: {actual} {limit_kind} exceeds the \
                 {limit} allowed — send fewer prompts per call, or shorten each one"
            ),
            Self::Unauthorized { model } => write!(
                f,
                "llm.call(\"{model}\"): the provider rejected the API credential, or none was \
                 available — the operator sets a valid key for this provider"
            ),
            Self::Transport { model, detail } => write!(
                f,
                "llm.call(\"{model}\"): transport error before any prompt ran: {detail} — no \
                 tokens were billed; retry the call"
            ),
        }
    }
}

impl std::error::Error for LlmCallError {}

impl LlmCallError {
    /// The model this dispatch was aimed at. Every variant names one, so a caller
    /// need not match to attribute the failure.
    pub fn model(&self) -> &str {
        match self {
            Self::NotConfigured { model }
            | Self::UnknownModel { model, .. }
            | Self::BudgetExceeded { model, .. }
            | Self::PromptBoundsExceeded { model, .. }
            | Self::Unauthorized { model }
            | Self::Transport { model, .. } => model,
        }
    }

    /// Token-budget refusals become catchable `QuotaExceededError`s at the
    /// stdlib boundary. Prompt size/count bounds remain `RangeError`s.
    pub fn is_budget_exceeded(&self) -> bool {
        matches!(self, Self::BudgetExceeded { .. })
    }
}

/// Why one *element* of a dispatch is not a clean completion.
///
/// Per-element, and orthogonal to [`LlmCallError`]: the request did reach the
/// model. Several of these still carry usable text on the [`LlmOutcome`] — a
/// truncated or content-filtered completion is `ok: false` and has a partial
/// answer worth reading.
///
/// A closed set of nine. It is what a guest branches on, so a provider that
/// meets something it cannot place reports [`Self::Incomplete`] and puts the raw
/// provider spelling in [`LlmFailure::finish_reason`] rather than widening this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// Hit the output-token ceiling mid-answer (`finishReason` of `length`). The
    /// partial text is retained — that is the whole reason the failure arm has a
    /// `text` field.
    Truncated,
    /// The provider's safety filter stopped or redacted the completion. Reached
    /// from both directions: `generateText` resolves with a filtered result while
    /// `generateObject` throws on the same input, and both normalize to here.
    ContentFiltered,
    /// The provider answered, but not with something usable: malformed output,
    /// or output that ignored the requested schema. Distinct from
    /// [`Self::Truncated`] — nothing was cut off, what arrived was wrong.
    InvalidOutput,
    /// The provider throttled this element (structurally, a 429). Honor any
    /// reported retry-after before re-dispatching.
    RateLimited,
    /// The provider refused the request itself rather than failing to answer it:
    /// a rejected credential, a malformed or oversized request, or a prompt that
    /// does not fit the model's context window. These share an arm because they
    /// share a shape — the request as sent will never succeed, so retrying it
    /// unchanged cannot help.
    RequestRejected,
    /// The provider was reachable but could not serve the request (structurally,
    /// a 5xx). Unlike [`Self::RequestRejected`] the request itself may be fine,
    /// so the same bytes may succeed later.
    ProviderUnavailable,
    /// The connection failed for this element after dispatch had begun — no HTTP
    /// status was ever observed. Sibling elements may still have succeeded.
    Transport,
    /// The element was abandoned before the model finished — deadline, shutdown,
    /// or an aborted request. Classified separately from [`Self::Transport`]
    /// because the cause is on this side, not the wire.
    Cancelled,
    /// The provider stopped for a reason this runtime does not classify. The raw
    /// provider spelling travels in [`LlmFailure::finish_reason`] for diagnosis.
    Incomplete,
}

impl FailureReason {
    /// The stable kebab-case spelling. This is the wire form: it is what any
    /// guest-facing rendering uses, so it must not drift.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::ContentFiltered => "content-filtered",
            Self::InvalidOutput => "invalid-output",
            Self::RateLimited => "rate-limited",
            Self::RequestRejected => "request-rejected",
            Self::ProviderUnavailable => "provider-unavailable",
            Self::Transport => "transport",
            Self::Cancelled => "cancelled",
            Self::Incomplete => "incomplete",
        }
    }

    /// A fixed classification string for this reason, suitable as an
    /// [`LlmFailure::message`] when a provider has nothing more specific that is
    /// safe to say. Drawn from a closed vocabulary precisely so it cannot carry a
    /// response body or completion fragment.
    pub fn default_message(&self) -> &'static str {
        match self {
            Self::Truncated => "the model stopped at the output token cap",
            Self::ContentFiltered => "the provider's safety filter stopped the completion",
            Self::InvalidOutput => "the model returned output that did not match the request",
            Self::RateLimited => "the provider throttled this request",
            Self::RequestRejected => "the provider refused the request as sent",
            Self::ProviderUnavailable => "the provider could not serve this request",
            Self::Transport => "the connection failed before a response arrived",
            Self::Cancelled => "the request was abandoned before the model finished",
            Self::Incomplete => "the model stopped for an unclassified reason",
        }
    }

    /// Whether re-dispatching the identical request could plausibly succeed.
    ///
    /// The default a provider starts from, not the last word: it may override
    /// per element (see [`LlmFailure::retryable`]) when it knows better — a 429
    /// carrying a retry-after is retryable, while one on an exhausted quota is
    /// not.
    pub fn retryable_by_default(&self) -> bool {
        match self {
            Self::RateLimited | Self::ProviderUnavailable | Self::Transport => true,
            // The request as sent will not start succeeding, and a cancellation
            // was someone's decision — retrying either is a guess, so the guest
            // opts in rather than the runtime assuming.
            Self::Truncated
            | Self::ContentFiltered
            | Self::InvalidOutput
            | Self::RequestRejected
            | Self::Cancelled
            | Self::Incomplete => false,
        }
    }

    /// Every reason, in the plan's declaration order. Kept so a mapper or a
    /// guest-facing enumeration cannot silently omit one.
    pub const ALL: [FailureReason; 9] = [
        Self::Truncated,
        Self::ContentFiltered,
        Self::InvalidOutput,
        Self::RateLimited,
        Self::RequestRejected,
        Self::ProviderUnavailable,
        Self::Transport,
        Self::Cancelled,
        Self::Incomplete,
    ];
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The failure-arm payload of an [`LlmOutcome`]: everything that exists only
/// when `ok` is false.
///
/// Held in an `Option` on the outcome rather than flattened into it, so the
/// success arm *cannot* carry a `message`, a `retryable`, or a `status` — those
/// fields are meaningless on a clean completion, and a flattened struct would
/// have to invent values for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmFailure {
    /// The closed-set classification a guest branches on.
    pub reason: FailureReason,
    /// The degraded error string.
    ///
    /// This is the AI SDK's rule that an `error: unknown` becomes an
    /// `errorText: string` at any serialization boundary — a guest cannot receive
    /// a Rust error object, so it receives this. **The no-payload rule binds it absolutely: it is
    /// a classification, never an echo.** No prompt, no completion fragment, and
    /// no provider response body may reach it; provider bodies carry request
    /// echoes and account identifiers, so they are dropped at this boundary.
    /// [`FailureReason::default_message`] supplies a safe one.
    pub message: String,
    /// Whether re-dispatching the identical request could plausibly succeed.
    /// Defaulted from [`FailureReason::retryable_by_default`] and overridable per
    /// element, because a provider sometimes knows more than the class does.
    pub retryable: bool,
    /// The HTTP status, when one was observed. `None` is meaningful rather than
    /// missing: a transport death never got a status, which is exactly what
    /// distinguishes it from a provider that answered with an error.
    pub status: Option<u16>,
    /// The provider's own raw stop reason, verbatim, for diagnosis only.
    ///
    /// **Deliberately separate from [`Self::reason`].** `reason` is the closed
    /// set a guest branches on; this is an open string that varies per provider
    /// and per SDK version. Folding them together would either force the closed
    /// set to grow with every provider quirk or silently discard the detail that
    /// makes an unclassified stop diagnosable.
    pub finish_reason: Option<String>,
}

impl LlmFailure {
    /// A failure of this class carrying this classification message. `retryable`
    /// starts from the reason's default; `status` and `finish_reason` start
    /// absent.
    pub fn new(reason: FailureReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
            retryable: reason.retryable_by_default(),
            status: None,
            finish_reason: None,
        }
    }

    /// Override the class default when the provider knows better.
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Record the observed HTTP status. Left absent when none was seen.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Record the provider's raw stop reason for diagnosis.
    pub fn with_finish_reason(mut self, finish_reason: impl Into<String>) -> Self {
        self.finish_reason = Some(finish_reason.into());
        self
    }
}

/// One element's result, positionally matched to the prompt that produced it.
///
/// A struct rather than an enum because success and failure are not disjoint
/// here: a truncated or content-filtered completion is `ok: false` and *still
/// carries usable text*, and a caller that wants the partial answer should not
/// have to reach through an error arm to get it.
///
/// `ok: true` means the provider reported a **natural stop** — not merely that no
/// error was raised. A completion that returned cleanly but stopped on `length`
/// is `ok: false` with [`FailureReason::Truncated`]. That choice trades a class
/// of silent wrong-answer bugs for one honest data-loss risk, which is why the
/// failure arm keeps its text rather than discarding it.
///
/// The arms differ in more than a flag: `text` is always present on success and
/// nullable on failure, and the [`LlmFailure`] payload exists only on failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmOutcome {
    /// The provider reported a natural stop for this element.
    pub ok: bool,
    /// The completion text.
    ///
    /// Always `Some` on the success arm. On the failure arm it is `string | null`:
    /// `Some` — including `Some("")` — when the model produced text, and `None`
    /// when it produced none at all. The two must not collapse, since telling
    /// "empty because filtered" from "empty because the model said nothing" is
    /// the distinction the `ok` rule exists to preserve.
    pub text: Option<String>,
    /// The failure payload. `Some` exactly when `!ok`.
    pub failure: Option<LlmFailure>,
    /// Tokens the provider reported for the prompt. `None` means
    /// **indeterminate, not free** — the provider reported nothing, or reported
    /// something non-finite. Never `Some(0)` as a stand-in for "unknown": a
    /// throttled call may still have been billed, so a reconciler holds a
    /// conservative reserve for `None` rather than releasing it.
    ///
    /// Nullable on *both* arms: a provider can resolve successfully with no usage
    /// reported, so this cannot be confined to the failure arm.
    pub input_tokens: Option<u64>,
    /// Tokens the provider reported for the completion. Same
    /// `None`-means-indeterminate rule as [`Self::input_tokens`].
    pub output_tokens: Option<u64>,
}

impl LlmOutcome {
    /// A natural stop with the given text. Usage is left indeterminate; a
    /// provider that reports counts sets them explicitly.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: Some(text.into()),
            failure: None,
            input_tokens: None,
            output_tokens: None,
        }
    }

    /// A non-clean element. `text` is whatever the model did produce: `Some` when
    /// it produced any, `None` when it produced none — retained rather than
    /// discarded, so a partial answer survives.
    pub fn failed(failure: LlmFailure, text: Option<impl Into<String>>) -> Self {
        Self {
            ok: false,
            text: text.map(Into::into),
            failure: Some(failure),
            input_tokens: None,
            output_tokens: None,
        }
    }

    /// Attach reported usage. A non-finite or unreported count stays `None`.
    pub fn with_usage(mut self, input_tokens: Option<u64>, output_tokens: Option<u64>) -> Self {
        self.input_tokens = input_tokens;
        self.output_tokens = output_tokens;
        self
    }
}

/// A model the provider serves.
///
/// Every optional may legitimately be `None`: models are blueprint-declared, and
/// an operator may declare a name without a description, a context window, or an
/// output reserve. Absent means unknown, not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmModel {
    pub name: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
}

impl LlmModel {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            context_window: None,
        }
    }
}

/// The embedder-provided model provider: resolves the credential, performs the
/// completions, and reports what each one cost.
///
/// Object-safe boxed-future trait (rather than `async fn`) so the interpreter
/// stays free of the embedder's async stack while still `await`-ing the call from
/// within the async host function — the shape
/// [`McpTransport`](crate::runtime::mcp::McpTransport) uses.
///
/// # Obligations on every implementor
///
/// These are *contractual*, not enforced by the interpreter. A second
/// implementor inherits them:
///
/// 1. **Positional ordering.** The returned `Vec<LlmOutcome>` has exactly one
///    element per input prompt, in input order. Element `i` is the outcome of
///    `prompts[i]`, including when it failed.
/// 2. **Bounded concurrency.** `call` takes a *slice* rather than a single
///    prompt because the guest is single-threaded Wasm holding one host call at a
///    time: fan-out cannot live above this boundary, so it lives below it. The
///    implementor is responsible for bounding that concurrency — the interpreter
///    does not limit it.
/// 3. **Per-element failure never becomes `Err`.** One bad element is an
///    `LlmOutcome` with `ok: false`. `Err(LlmCallError)` is only for dispatch
///    that never ran, or a transport that died before any element started; an
///    `Err` discards the siblings that did succeed and were billed.
/// 4. **No prompt or completion text in errors.** Nothing an implementor puts in
///    an [`LlmCallError`] may quote the payload.
pub trait LlmProvider: Send + Sync {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        schema_json: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>>;

    /// Returns the locally declared model catalog without network I/O or other
    /// side effects. The caller must inspect these names before it can check
    /// policy for each model, so this runs before per-model authorization.
    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>>;

    /// The output cap declared for one model, consulted *before* dispatch.
    ///
    /// Separate from [`Self::models`] because the reservation needs one model's
    /// reserve on the hot path of every `call`; using the whole catalog here
    /// would add unnecessary work to each dispatch. An implementor that already
    /// holds its declarations (the blueprint case) answers from memory, and one
    /// that does not should return `None` rather than block.
    ///
    /// `None` means the model declared no reserve, or the implementor cannot
    /// answer cheaply; the caller then applies
    /// [`LlmLimits::default_output_cap`]. Returning `None` is always safe, which
    /// is why this defaults rather than being required of every implementor.
    fn output_reserve(&self, _model: &str) -> Option<u64> {
        None
    }
}

/// One execution's token budget, reserving against its own ceiling and the
/// server-wide one together.
///
/// Both reservations are taken together and both unwound if either fails, so a
/// refused dispatch leaves neither counter charged.
pub struct ExecutionTokenBudget {
    limits: LlmLimits,
    ledger: TokenLedger,
}

impl ExecutionTokenBudget {
    pub fn new(limits: LlmLimits, aggregate: SharedTokenBudget) -> Self {
        let ledger = TokenLedger::new(
            LedgerLimits {
                per_execution_tokens: limits.per_execution_tokens,
                max_held_tokens: limits.max_held_tokens,
            },
            aggregate,
        );
        Self { limits, ledger }
    }

    pub fn limits(&self) -> LlmLimits {
        self.limits
    }

    /// Tokens this execution currently holds against its ceiling.
    pub fn used(&self) -> u64 {
        self.ledger.used()
    }

    /// The part of [`Self::used`] held for unreported usage.
    pub fn held(&self) -> u64 {
        self.ledger.held()
    }

    /// Reserve `tokens` against this execution and the aggregate, before
    /// dispatch.
    ///
    /// The caller passes the full reservation — `estimated_input_tokens +
    /// (output_cap × prompt_count)`, via [`Self::reservation_for`] — because
    /// reserving only input enforces the ceiling retroactively: output tokens
    /// are typically the expensive half, and a small prompt can legitimately
    /// produce a very large completion with nothing standing in the way.
    pub fn reserve(&self, model: &str, tokens: u64) -> Result<(), LlmCallError> {
        self.ledger.reserve(tokens).map_err(|refusal| {
            let limit_kind = match refusal {
                LedgerRefusal::HeldReserve { held, limit } => {
                    LlmLimitKind::IndeterminateSpend { held, limit }
                }
                LedgerRefusal::PerExecution { requested, limit } => {
                    LlmLimitKind::PerExecutionTokens { requested, limit }
                }
                LedgerRefusal::AllExecutions { requested, limit } => {
                    LlmLimitKind::AllExecutionsTokens { requested, limit }
                }
            };
            LlmCallError::BudgetExceeded {
                model: model.to_string(),
                limit_kind,
            }
        })
    }

    /// The reservation a dispatch of `prompt_count` prompts needs:
    /// `estimated_input_tokens + (output_cap × prompt_count)`.
    ///
    /// `output_cap` is the resolved per-model `outputReserve` when the model
    /// declares one, and [`LlmLimits::default_output_cap`] otherwise. The same
    /// cap is passed to the provider as the request's `maxOutputTokens`, which
    /// makes the reservation an actual upper bound rather than an estimate.
    pub fn reservation_for(
        &self,
        estimated_input_tokens: u64,
        prompt_count: u64,
        output_cap: Option<u64>,
    ) -> u64 {
        let cap = output_cap.unwrap_or(self.limits.default_output_cap);
        estimated_input_tokens.saturating_add(cap.saturating_mul(prompt_count))
    }

    /// Reconcile a completed dispatch down from its `reserved` estimate to what
    /// the provider actually reported.
    ///
    /// `reported` is the summed usage of the elements the provider gave counts
    /// for; `indeterminate` is the reserve belonging to elements it reported
    /// nothing for. The indeterminate part is **not** released — `None` means
    /// indeterminate, not free, since a throttled call may still have been
    /// billed — and is instead carried as held reserve, bounded by
    /// [`LlmLimits::max_held_tokens`].
    pub fn reconcile(&self, reserved: u64, reported: u64, indeterminate: u64) {
        self.ledger.reconcile(reserved, reported, indeterminate);
    }

    /// Release a reservation for prompts that never dispatched — the remainder
    /// of a batch abandoned partway.
    pub fn release(&self, tokens: u64) {
        self.reconcile(tokens, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        ExecutionTokenBudget, FailureReason, LlmCallError, LlmFailure, LlmLimitKind, LlmLimits,
        LlmModel, LlmOutcome, LlmProvider, PromptBoundKind, SharedTokenBudget,
    };

    const MODEL: &str = "claude-sonnet-4";

    /// An execution ends by having its budget holder dropped, not by reconciling
    /// every reservation first. Without the release on drop the reservation of
    /// every ended execution would be held forever and the server-wide budget
    /// would ratchet to its cap, refusing every later execution.
    #[test]
    fn dropping_an_execution_budget_returns_its_tokens_to_the_aggregate() {
        let aggregate = SharedTokenBudget::new(1_000);
        {
            let held = ExecutionTokenBudget::new(limits_with_cap(1_000), aggregate.clone());
            held.reserve(MODEL, 1_000)
                .expect("the first execution fits");
            assert_eq!(aggregate.used(), 1_000);
        }
        assert_eq!(
            aggregate.used(),
            0,
            "the ended execution released its tokens"
        );

        let next = ExecutionTokenBudget::new(limits_with_cap(1_000), aggregate);
        next.reserve(MODEL, 1_000)
            .expect("a later execution reuses the capacity");
    }

    fn limits_with_cap(per_execution_tokens: u64) -> LlmLimits {
        LlmLimits {
            per_execution_tokens,
            ..LlmLimits::default()
        }
    }

    /// A panicking execution must still return its tokens: an unrelated panic
    /// while the lock was held would otherwise strand the whole reservation, and
    /// the aggregate would ratchet exactly as it does without any release.
    #[test]
    fn a_poisoned_lock_still_releases_on_drop() {
        let aggregate = SharedTokenBudget::new(1_000);
        {
            let held = ExecutionTokenBudget::new(limits_with_cap(1_000), aggregate.clone());
            held.reserve(MODEL, 600).expect("reservation fits");

            let poisoner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = held.ledger.lock_state().expect("lock");
                panic!("poison the budget's lock");
            }));
            assert!(poisoner.is_err(), "the panic must have unwound");
            assert!(held.ledger.state_is_poisoned(), "the lock must be poisoned");
        }
        assert_eq!(
            aggregate.used(),
            0,
            "a poisoned execution still returns its tokens"
        );
    }

    /// Two executions reserving at once must not both observe the same headroom.
    /// With `fetch_add`-and-compensate both can read the pre-add value and both
    /// conclude they fit; the CAS loop makes exactly one win.
    #[test]
    fn concurrent_reservations_cannot_both_claim_the_same_headroom() {
        // Many rounds with many threads: a race that resolves correctly once may
        // only have been serialized by luck, and two threads rarely collide
        // inside the few instructions between the load and the store.
        const THREADS: u64 = 16;
        const ROUNDS: usize = 500;

        let mut overspends = 0;
        for _ in 0..ROUNDS {
            // Headroom for exactly one of the racing reservations.
            let aggregate = SharedTokenBudget::new(1_000);
            let start = Arc::new(std::sync::Barrier::new(THREADS as usize));
            let winners = Arc::new(AtomicU64::new(0));

            let threads: Vec<_> = (0..THREADS)
                .map(|_| {
                    let aggregate = aggregate.clone();
                    let start = Arc::clone(&start);
                    let winners = Arc::clone(&winners);
                    std::thread::spawn(move || {
                        let budget =
                            ExecutionTokenBudget::new(limits_with_cap(1_000), aggregate.clone());
                        // Release every thread into the reservation together.
                        start.wait();
                        if budget.reserve(MODEL, 1_000).is_ok() {
                            winners.fetch_add(1, Ordering::Relaxed);
                        }
                        // Hold every reservation until the round is over, so a
                        // win is never handed back before it is contested.
                        std::mem::forget(budget);
                    })
                })
                .collect();
            for t in threads {
                t.join().expect("reserving thread");
            }

            let winners = winners.load(Ordering::Relaxed);
            assert_eq!(
                winners, 1,
                "exactly one of {THREADS} racing reservations may claim the only headroom"
            );
            if aggregate.used() > aggregate.cap() {
                overspends += 1;
            }
        }
        assert_eq!(
            overspends, 0,
            "the aggregate was overdrawn in {overspends}/{ROUNDS} rounds"
        );
    }

    /// The two token ceilings refuse for opposite reasons and want opposite
    /// responses: make fewer or shorter calls, versus wait or ask an operator. A
    /// message that reads the same for both sends a program — or the model
    /// writing it — to shrink work that was never what filled the budget.
    #[test]
    fn the_per_execution_and_server_wide_token_limits_name_different_fixes() {
        let roomy = SharedTokenBudget::new(u64::MAX);
        let own = ExecutionTokenBudget::new(limits_with_cap(100), roomy)
            .reserve(MODEL, 500)
            .expect_err("over this execution's own ceiling")
            .to_string();

        // An aggregate with no headroom left by another execution, so this one is
        // refused without ever approaching its own per-execution ceiling.
        let aggregate = SharedTokenBudget::new(100);
        let neighbour = ExecutionTokenBudget::new(limits_with_cap(u64::MAX), aggregate.clone());
        neighbour.reserve(MODEL, 100).expect("neighbour fits");
        let shared = ExecutionTokenBudget::new(limits_with_cap(u64::MAX), aggregate);
        let server_wide = shared
            .reserve(MODEL, 100)
            .expect_err("over the server-wide budget")
            .to_string();

        assert_ne!(own, server_wide);
        assert!(own.contains("this execution may spend"), "{own}");
        assert!(own.contains("use fewer or shorter calls"), "{own}");
        assert!(
            !own.contains("--max-llm-tokens"),
            "the per-execution ceiling is not the operator's flag: {own}"
        );
        assert!(
            server_wide.contains("across all live executions"),
            "{server_wide}"
        );
        assert!(
            server_wide.contains("is not what is in the way"),
            "{server_wide}"
        );
        assert!(
            server_wide.contains("--max-llm-tokens"),
            "the aggregate refusal must name the flag that raises it: {server_wide}"
        );
    }

    /// Each refusal must name the limit it hit and the ceiling, so an operator
    /// reading a log knows which knob is in play.
    #[test]
    fn exceeding_either_ceiling_names_the_limit_and_the_ceiling_hit() {
        let aggregate = SharedTokenBudget::new(u64::MAX);
        let budget = ExecutionTokenBudget::new(limits_with_cap(64), aggregate);
        let err = budget.reserve(MODEL, 4_096).expect_err("over the ceiling");
        assert!(
            matches!(
                err,
                LlmCallError::BudgetExceeded {
                    limit_kind: LlmLimitKind::PerExecutionTokens { .. },
                    ..
                }
            ),
            "{err:?}"
        );
        let rendered = err.to_string();
        assert!(rendered.contains(MODEL), "{rendered}");
        assert!(rendered.contains("4096"), "the request: {rendered}");
        assert!(rendered.contains("64"), "the ceiling: {rendered}");
    }

    /// a budget refusal carries the numbers, never the payload.
    #[test]
    fn no_budget_error_display_leaks_prompt_text() {
        const SECRET: &str = "the patient's diagnosis is";
        let aggregate = SharedTokenBudget::new(10);
        let neighbour = ExecutionTokenBudget::new(limits_with_cap(u64::MAX), aggregate.clone());
        neighbour.reserve(MODEL, 10).expect("neighbour fits");

        let rendered: Vec<String> = [
            ExecutionTokenBudget::new(limits_with_cap(1), SharedTokenBudget::new(u64::MAX))
                .reserve(MODEL, 900)
                .expect_err("per-execution"),
            ExecutionTokenBudget::new(limits_with_cap(u64::MAX), aggregate)
                .reserve(MODEL, 900)
                .expect_err("server-wide"),
            held_budget_at_its_ceiling()
                .reserve(MODEL, 1)
                .expect_err("indeterminate"),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();

        for message in &rendered {
            assert!(!message.contains(SECRET), "{message}");
            assert!(!message.contains("patient"), "{message}");
        }
    }

    /// An execution whose held reserve is already over the bound.
    fn held_budget_at_its_ceiling() -> ExecutionTokenBudget {
        let budget = ExecutionTokenBudget::new(
            LlmLimits {
                per_execution_tokens: u64::MAX,
                max_held_tokens: 100,
                ..LlmLimits::default()
            },
            SharedTokenBudget::new(u64::MAX),
        );
        budget.reserve(MODEL, 500).expect("the dispatch fits");
        // Every element came back with no usage reported, so the whole
        // reservation is carried as held rather than released.
        budget.reconcile(500, 0, 500);
        budget
    }

    /// A batch abandoned partway must not keep charging for the prompts that
    /// never dispatched.
    #[test]
    fn an_undispatched_remainder_is_released_after_a_partial_batch() {
        let aggregate = SharedTokenBudget::new(1_000);
        let budget = ExecutionTokenBudget::new(limits_with_cap(1_000), aggregate.clone());
        budget.reserve(MODEL, 900).expect("the whole batch fits");

        // Four of ten prompts ran and reported 120 tokens; the rest never
        // dispatched, so their share of the reservation is released.
        budget.reconcile(900, 120, 0);

        assert_eq!(budget.used(), 120);
        assert_eq!(aggregate.used(), 120, "the remainder returned");
        budget
            .reserve(MODEL, 800)
            .expect("the released capacity is reusable");
    }

    /// reserving only input enforces the ceiling retroactively. A
    /// 100-token prompt can legitimately produce 100k output tokens, so the
    /// output cap must be part of the reservation and the refusal must land
    /// before dispatch, not after the provider has billed.
    #[test]
    fn the_reservation_covers_output_tokens_and_refuses_before_dispatch() {
        let aggregate = SharedTokenBudget::new(u64::MAX);
        let budget = ExecutionTokenBudget::new(
            LlmLimits {
                per_execution_tokens: 10_000,
                default_output_cap: 64_000,
                ..LlmLimits::default()
            },
            aggregate.clone(),
        );

        // Input alone is trivially under the ceiling.
        let input_only = 100;
        assert!(input_only < budget.limits().per_execution_tokens);

        let reservation = budget.reservation_for(input_only, 1, None);
        assert_eq!(
            reservation,
            100 + 64_000,
            "output cap is in the reservation"
        );

        let err = budget
            .reserve(MODEL, reservation)
            .expect_err("refused before dispatch, not after the provider billed");
        assert!(
            matches!(
                err,
                LlmCallError::BudgetExceeded {
                    limit_kind: LlmLimitKind::PerExecutionTokens { .. },
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(budget.used(), 0, "a refused dispatch charges nothing");
        assert_eq!(aggregate.used(), 0);

        // The cap scales with the prompt count, and a model's own reserve wins
        // over the default.
        assert_eq!(budget.reservation_for(100, 4, None), 100 + 64_000 * 4);
        assert_eq!(budget.reservation_for(100, 4, Some(1_000)), 100 + 4_000);
    }

    /// held reserve releases from the per-execution budget on teardown —
    /// that execution is over — but stays charged against the aggregate.
    /// Releasing it from the aggregate would let indeterminate spend escape the
    /// server-wide ceiling: N executions each under the per-execution ceiling
    /// could exceed the server-wide one without ever tripping it.
    #[test]
    fn a_held_reserve_commits_against_the_aggregate_and_releases_only_per_execution() {
        let aggregate = SharedTokenBudget::new(1_500);
        let limits = LlmLimits {
            per_execution_tokens: 400,
            max_held_tokens: 10_000,
            ..LlmLimits::default()
        };

        // Three executions that each spend 400 tokens the provider never reports.
        for _ in 0..3 {
            let budget = ExecutionTokenBudget::new(limits, aggregate.clone());
            budget.reserve(MODEL, 400).expect("within its own ceiling");
            budget.reconcile(400, 0, 400);
            assert_eq!(budget.held(), 400);
        }

        assert_eq!(
            aggregate.used(),
            1_200,
            "indeterminate spend stays charged against the aggregate after teardown"
        );

        // A fourth execution, itself well under the per-execution ceiling, is
        // now refused by the server-wide ceiling it would otherwise have walked
        // straight past.
        let next = ExecutionTokenBudget::new(limits, aggregate);
        let err = next.reserve(MODEL, 400).expect_err("the aggregate refuses");
        assert!(
            matches!(
                err,
                LlmCallError::BudgetExceeded {
                    limit_kind: LlmLimitKind::AllExecutionsTokens { .. },
                    ..
                }
            ),
            "{err:?}"
        );
    }

    /// Token spend is monotonic, so a held estimate is never superseded by a real
    /// number and a long-lived execution accumulating null usage would ratchet to
    /// zero with no recovery. The bound names *indeterminate spend*, distinctly
    /// from a genuine overspend, so an operator can tell a null-usage storm from
    /// real consumption.
    #[test]
    fn crossing_the_held_reserve_bound_is_distinguishable_from_a_genuine_overspend() {
        let held = held_budget_at_its_ceiling();
        let indeterminate = held
            .reserve(MODEL, 1)
            .expect_err("the held bound is crossed");
        assert!(
            matches!(
                indeterminate,
                LlmCallError::BudgetExceeded {
                    limit_kind: LlmLimitKind::IndeterminateSpend { .. },
                    ..
                }
            ),
            "{indeterminate:?}"
        );

        let overspend =
            ExecutionTokenBudget::new(limits_with_cap(10), SharedTokenBudget::new(1_000))
                .reserve(MODEL, 900)
                .expect_err("a genuine overspend");

        let indeterminate = indeterminate.to_string();
        let overspend = overspend.to_string();
        assert_ne!(indeterminate, overspend);
        assert!(
            indeterminate.contains("indeterminate spend ceiling reached"),
            "{indeterminate}"
        );
        assert!(
            indeterminate.contains("never reported usage"),
            "the operator must be able to tell unreported spend from measured: {indeterminate}"
        );
        assert!(
            !overspend.contains("indeterminate"),
            "a real overspend must not read as a null-usage storm: {overspend}"
        );
    }

    /// A ceiling is something a program can catch and adapt to, so it reaches the
    /// guest as a catchable `QuotaExceededError`. A dispatch that failed for
    /// a reason retrying smaller cannot fix
    /// must not masquerade as one.
    #[test]
    fn only_quota_refusals_are_classified_as_quota_exceeded_errors() {
        for error in every_variant() {
            let expected = matches!(error, LlmCallError::BudgetExceeded { .. });
            assert_eq!(
                error.is_budget_exceeded(),
                expected,
                "{error:?} is misclassified for the guest boundary"
            );
        }

        // Every token ceiling, not just the one in `every_variant`.
        for limit_kind in [
            LlmLimitKind::PerExecutionTokens {
                requested: 2,
                limit: 1,
            },
            LlmLimitKind::AllExecutionsTokens {
                requested: 2,
                limit: 1,
            },
            LlmLimitKind::IndeterminateSpend { held: 2, limit: 1 },
        ] {
            let error = LlmCallError::BudgetExceeded {
                model: MODEL.to_string(),
                limit_kind,
            };
            assert!(error.is_budget_exceeded(), "{error:?}");
        }
    }

    /// The prompt bounds U4 enforces live alongside the token ceilings so an
    /// operator raising one raises the other in one place.
    #[test]
    fn default_limits_carry_both_token_ceilings_and_prompt_bounds() {
        let limits = LlmLimits::default();
        assert_eq!(limits.max_prompt_count, 128);
        assert_eq!(limits.max_prompt_bytes, 256 * 1024);
        assert_eq!(
            limits.per_execution_tokens,
            super::DEFAULT_MAX_EXECUTION_TOKENS
        );
        assert_eq!(limits.default_output_cap, super::DEFAULT_OUTPUT_CAP);
    }

    /// One of every dispatch-level variant, so a new variant fails to compile
    /// here until it is added.
    fn every_variant() -> Vec<LlmCallError> {
        vec![
            LlmCallError::NotConfigured {
                model: MODEL.to_string(),
            },
            LlmCallError::UnknownModel {
                model: MODEL.to_string(),
                available: vec!["claude-haiku-4".to_string()],
            },
            LlmCallError::BudgetExceeded {
                model: MODEL.to_string(),
                limit_kind: LlmLimitKind::PerExecutionTokens {
                    requested: 4_000,
                    limit: 120,
                },
            },
            LlmCallError::PromptBoundsExceeded {
                model: MODEL.to_string(),
                limit_kind: PromptBoundKind::PromptCount,
                actual: 64,
                limit: 32,
            },
            LlmCallError::Unauthorized {
                model: MODEL.to_string(),
            },
            LlmCallError::Transport {
                model: MODEL.to_string(),
                detail: "connection reset".to_string(),
            },
        ]
    }

    #[test]
    fn six_dispatch_variants_each_name_the_model_and_a_distinct_fix() {
        let variants = every_variant();
        assert_eq!(variants.len(), 6, "LlmCallError has six dispatch variants");

        let mut fixes: Vec<String> = Vec::new();
        for err in &variants {
            let rendered = err.to_string();
            assert!(
                rendered.contains(MODEL),
                "variant must name the model: {rendered}"
            );
            assert_eq!(err.model(), MODEL);
            let (_, fix) = rendered.split_once(" — ").unwrap_or_else(|| {
                panic!("variant must suggest a fix after an em dash: {rendered}")
            });
            assert!(!fix.trim().is_empty(), "empty fix: {rendered}");
            fixes.push(fix.to_string());
        }

        // Distinct fixes: a shared message would tell the caller nothing about
        // which of the six they hit.
        let mut sorted = fixes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            fixes.len(),
            "each variant needs its own fix: {fixes:?}"
        );
    }

    /// no prompt or completion text may reach an error `Display`.
    #[test]
    fn no_dispatch_error_display_leaks_prompt_text() {
        const SECRET_PROMPT: &str = "patient SSN is 000-00-0000";
        const SECRET_COMPLETION: &str = "the diagnosis is confidential";

        for err in every_variant() {
            let rendered = err.to_string();
            assert!(
                !rendered.contains(SECRET_PROMPT),
                "prompt text leaked into Display: {rendered}"
            );
            assert!(
                !rendered.contains(SECRET_COMPLETION),
                "completion text leaked into Display: {rendered}"
            );
        }

        // The only free-form field any variant carries is `Transport { detail }`,
        // which is a transport diagnostic and never built from a payload. Prove
        // no *other* variant has a construction path that could carry one by
        // showing the rendered set is otherwise fully determined by model name
        // and numbers.
        let bounded = LlmCallError::PromptBoundsExceeded {
            model: MODEL.to_string(),
            limit_kind: PromptBoundKind::PromptBytes,
            actual: 900_000,
            limit: 65_536,
        };
        let rendered = bounded.to_string();
        assert!(rendered.contains("900000") && rendered.contains("65536"));
        assert!(
            rendered.contains("bytes in a single prompt"),
            "bound kind must be named: {rendered}"
        );
    }

    /// nine, in the plan's declaration order. The spellings are the wire
    /// form a guest branches on, so drift here is a guest-visible break.
    #[test]
    fn nine_failure_reasons_have_stable_kebab_case_spellings() {
        let spellings: Vec<&str> = FailureReason::ALL
            .iter()
            .map(FailureReason::as_str)
            .collect();
        assert_eq!(
            spellings,
            [
                "truncated",
                "content-filtered",
                "invalid-output",
                "rate-limited",
                "request-rejected",
                "provider-unavailable",
                "transport",
                "cancelled",
                "incomplete",
            ]
        );
        for reason in FailureReason::ALL {
            assert_eq!(reason.to_string(), reason.as_str());
        }
    }

    /// `ok` keys off a natural stop, not off "didn't throw"; absent usage is
    /// `None`, never `Some(0)`; and the failure arm keeps partial text.
    #[test]
    fn truncated_outcome_is_not_ok_but_keeps_its_text_and_indeterminate_usage() {
        let partial = LlmOutcome::failed(
            LlmFailure::new(FailureReason::Truncated, "output token ceiling reached"),
            Some("the answer begins"),
        );
        assert!(!partial.ok, "a length stop is not a natural stop");
        assert_eq!(
            partial.text.as_deref(),
            Some("the answer begins"),
            "partial text is retained"
        );
        let failure = partial.failure.as_ref().expect("failure arm present");
        assert_eq!(failure.reason, FailureReason::Truncated);
        assert_eq!(
            partial.input_tokens, None,
            "unreported usage is indeterminate"
        );
        assert_eq!(partial.output_tokens, None);

        let clean = LlmOutcome::success("done").with_usage(Some(12), Some(0));
        assert!(clean.ok);
        assert!(clean.failure.is_none(), "a natural stop carries no reason");
        assert_eq!(
            clean.output_tokens,
            Some(0),
            "a real zero is Some(0); only absence is None"
        );
    }

    /// `text` is `string | null` on the failure arm specifically — a model
    /// that produced nothing is distinguishable from one that produced "".
    #[test]
    fn failure_text_is_nullable_and_distinct_from_empty() {
        let nothing = LlmOutcome::failed(
            LlmFailure::new(FailureReason::ContentFiltered, "filtered before any output"),
            None::<String>,
        );
        assert_eq!(nothing.text, None, "produced nothing at all");

        let empty = LlmOutcome::failed(
            LlmFailure::new(
                FailureReason::ContentFiltered,
                "filtered after an empty turn",
            ),
            Some(""),
        );
        assert_eq!(empty.text.as_deref(), Some(""), "produced an empty string");
        assert_ne!(
            nothing.text, empty.text,
            "null and empty must not collapse — the whole point is telling \
             'empty because filtered' from 'empty because it said nothing'"
        );
    }

    /// the failure arm carries `message`, `retryable`, and `status`, and the
    /// success arm structurally cannot.
    #[test]
    fn failure_arm_carries_message_retryable_and_nullable_status() {
        let throttled =
            LlmFailure::new(FailureReason::RateLimited, "provider throttled the request")
                .retryable(true)
                .with_status(429);
        assert_eq!(throttled.message, "provider throttled the request");
        assert!(throttled.retryable);
        assert_eq!(throttled.status, Some(429));

        // `status: number | null` — a transport death has no HTTP status at all.
        // That absence is the classification: it is exactly what distinguishes a
        // dead connection from a provider that answered with an error.
        let dead = LlmFailure::new(
            FailureReason::Transport,
            "connection closed before response",
        );
        assert_eq!(dead.status, None);

        // `retryable` defaults per class rather than uniformly: a dead connection
        // may succeed on the same bytes, while a refused request will not.
        assert!(dead.retryable, "transport is retryable by class");
        let refused = LlmFailure::new(FailureReason::RequestRejected, "request refused as sent");
        assert!(
            !refused.retryable,
            "a request the provider refused will not start succeeding unchanged"
        );
        // ...and the class default is a starting point, not the last word.
        assert!(
            !LlmFailure::new(FailureReason::RateLimited, "quota exhausted")
                .retryable(false)
                .retryable,
            "a provider that knows better overrides the class default"
        );

        // Raw `finishReason` is a *diagnostic* field, separate from `reason`,
        // which is what guests branch on. They must not fold together.
        let truncated = LlmFailure::new(FailureReason::Truncated, "stopped at the output cap")
            .with_finish_reason("length");
        assert_eq!(truncated.reason, FailureReason::Truncated);
        assert_eq!(truncated.finish_reason.as_deref(), Some("length"));

        // A provider reporting a stop reason we do not classify still surfaces the
        // raw string for diagnosis while `reason` stays in the closed set.
        let odd = LlmFailure::new(FailureReason::Incomplete, "unclassified stop")
            .with_finish_reason("surprise-reason");
        assert_eq!(odd.reason, FailureReason::Incomplete);
        assert_eq!(odd.finish_reason.as_deref(), Some("surprise-reason"));
    }

    /// The retryable split follows the spike's structural classification: 429 and
    /// 5xx and a dead connection may succeed on the same bytes; everything else
    /// describes a request that will not start succeeding unchanged.
    #[test]
    fn retryable_defaults_split_transient_classes_from_terminal_ones() {
        let retryable: Vec<&str> = FailureReason::ALL
            .iter()
            .filter(|r| r.retryable_by_default())
            .map(FailureReason::as_str)
            .collect();
        assert_eq!(
            retryable,
            ["rate-limited", "provider-unavailable", "transport"]
        );
    }

    /// unlike `McpCallError::Upstream`, which renders `HTTP {status}:
    /// {body}`, no variant here interpolates a provider response body — those
    /// carry completion fragments and request echoes.
    #[test]
    fn no_dispatch_error_display_leaks_a_provider_response_body() {
        const BODY: &str = r#"{"error":{"message":"your prompt was: summarize the memo"}}"#;
        // The only free-form field in the taxonomy. A provider impl is
        // contracted to pass a fixed classification string; prove that when it
        // does, nothing body-shaped can reach the rendered message.
        let classified = LlmCallError::Transport {
            model: MODEL.to_string(),
            detail: "connection reset".to_string(),
        };
        let rendered = classified.to_string();
        assert!(!rendered.contains(BODY));
        assert!(!rendered.contains("summarize the memo"));
        assert!(!rendered.contains('{'), "no JSON body shape: {rendered}");
    }

    /// The degraded-error string: `message` follows the AI SDK's
    /// `error: unknown` → `errorText: string` rule, and carries no payload.
    #[test]
    fn failure_message_is_a_classification_not_a_payload_echo() {
        // Every message a provider impl is expected to produce is drawn from a
        // fixed classification vocabulary, so none can carry a response body.
        for reason in FailureReason::ALL {
            let failure = LlmFailure::new(reason, reason.default_message());
            assert!(
                !failure.message.is_empty(),
                "{reason} needs a message: it is the degraded error string"
            );
            assert!(
                !failure.message.contains("patient SSN"),
                "classification must not echo a payload: {}",
                failure.message
            );
        }
    }

    #[test]
    fn llm_model_optionals_may_both_be_absent() {
        let declared = LlmModel::new("operator-declared");
        assert_eq!(declared.description, None);
        assert_eq!(declared.context_window, None);
    }

    struct NoopProvider;

    impl LlmProvider for NoopProvider {
        fn call<'a>(
            &'a self,
            model: &'a str,
            prompts: &'a [String],
            _schema_json: Option<&'a str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a,
            >,
        > {
            let n = prompts.len();
            let model = model.to_string();
            Box::pin(async move {
                if model.is_empty() {
                    return Err(LlmCallError::NotConfigured { model });
                }
                // Obligation 1: one outcome per prompt, in order.
                Ok((0..n).map(|i| LlmOutcome::success(i.to_string())).collect())
            })
        }

        fn models<'a>(
            &'a self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>,
        > {
            Box::pin(async { Ok(vec![LlmModel::new("noop")]) })
        }
    }

    /// The trait must stay object-safe: the store holds it as `Arc<dyn ...>`.
    #[tokio::test]
    async fn provider_trait_is_object_safe_behind_arc() {
        let provider: Arc<dyn LlmProvider> = Arc::new(NoopProvider);
        let prompts = vec!["a".to_string(), "b".to_string()];
        let outcomes = provider
            .call(MODEL, &prompts, None)
            .await
            .expect("dispatch succeeds");
        assert_eq!(outcomes.len(), prompts.len(), "positional: one per prompt");
        assert_eq!(outcomes[1].text.as_deref(), Some("1"));
        assert_eq!(provider.models().await.unwrap()[0].name, "noop");
    }

    #[tokio::test]
    async fn unconfigured_store_data_has_no_provider() {
        // The default runtime leaves the provider absent so an unconfigured
        // call reports a catchable configuration error rather than silently
        // succeeding — the `session_kv` rule.
        let data = crate::runtime::StoreData::with_tempdir().unwrap();
        assert!(data.llm_provider.is_none());
    }
}
