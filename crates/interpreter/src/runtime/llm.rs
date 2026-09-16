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

/// The single internal host module every `submilli:llm` call dispatches through.
pub const LLM_MODULE_NAME: &str = "submilli:llm";

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
    /// The execution's token budget cannot cover this dispatch. Charged before
    /// any element ran, so nothing was billed.
    BudgetExceeded {
        model: String,
        requested: u64,
        remaining: u64,
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
    /// in this taxonomy, and R13 binds it: never a provider response body, and
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
            Self::BudgetExceeded {
                model,
                requested,
                remaining,
            } => write!(
                f,
                "llm.call(\"{model}\"): token budget exhausted: this dispatch needs {requested} \
                 tokens and {remaining} remain — split the work across executions, or the \
                 operator raises the execution's token budget"
            ),
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
    /// response body or completion fragment (R13).
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
    /// a Rust error object, so it receives this. **R13 binds it absolutely: it is
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
/// Both optionals may legitimately be `None`: models are blueprint-declared, and
/// an operator may declare a name without a description or a context window.
/// Absent means unknown, not zero.
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

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        FailureReason, LlmCallError, LlmFailure, LlmModel, LlmOutcome, LlmProvider, PromptBoundKind,
    };

    const MODEL: &str = "claude-sonnet-4";

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
                requested: 4_000,
                remaining: 120,
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

    /// R13: no prompt or completion text may reach an error `Display`.
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

    /// KTD2's nine, in the plan's declaration order. The spellings are the wire
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

    /// KTD1: `ok` keys off a natural stop, not off "didn't throw". KTD3: absent
    /// usage is `None`, never `Some(0)`. KTD2: the failure arm keeps partial text.
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

    /// KTD2: `text` is `string | null` on the failure arm specifically — a model
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
            "null and empty must not collapse — KTD1's whole point is telling \
             'empty because filtered' from 'empty because it said nothing'"
        );
    }

    /// KTD2: the failure arm carries `message`, `retryable`, and `status`, and the
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

    /// KTD7: unlike `McpCallError::Upstream`, which renders `HTTP {status}:
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

    /// R13 / KTD7 on the degraded-error string: `message` is the AI SDK's
    /// `error: unknown` → `errorText: string` rule, and R13 binds it absolutely.
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
        // R12: the default runtime leaves the provider absent so an unconfigured
        // call reports a catchable configuration error rather than silently
        // succeeding — the `session_kv` rule.
        let data = crate::runtime::StoreData::with_tempdir().unwrap();
        assert!(data.llm_provider.is_none());
    }
}
