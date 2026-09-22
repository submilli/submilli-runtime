//! The blueprint-backed [`LlmProvider`]: classify what a model dispatch came
//! back as, fan a batch out at a bounded concurrency, and report the declared
//! model catalog.
//!
//! **What this file does not do.** `models()` is a map over the blueprint this
//! impl already holds — no HTTP client, no timeout, no cache, no
//! unreachable-provider degradation path. That is not a simplification of MCP's
//! `discovery.rs`: no provider package exposes a model-listing API at all, so
//! the blueprint carries what MCP gets off the wire.
//!
//! **Classification runs in a fixed order**, numbered at each step below.
//! Reordering changes answers rather than just style: a retry wrapper hides the
//! HTTP status until it is unwrapped, an abort never appears wrapped at all, and
//! the structured-output path never consults the stop reason — so a step that
//! runs too late classifies a different failure than the one that happened.
//!
//! **No response body and no completion text leaves this module.** Step 5 reads
//! a response body to classify, so the body is in hand exactly where the error is
//! built — and the transport this file otherwise mirrors *does* interpolate one
//! (`McpCallError::Upstream` renders `HTTP {status}: {body}`). This one must not:
//! provider bodies carry completion fragments, request echoes, and account
//! identifiers, and a structured-output error's `text` field is raw model output
//! by definition. Both are dropped at the taxonomy boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use interpreter::runtime::{
    FailureReason, LlmCallError, LlmFailure, LlmModel, LlmOutcome, LlmProvider,
};
use submilli_blueprint::Blueprint;
use tokio::sync::Semaphore;

/// Elements dispatched at once. Chosen rather than inherited: a 429 is the one
/// structurally-detectable provider error and unbounded fan-out manufactures it,
/// retry-after can only be honored by a bounded pool, and retry wrapping already
/// means each element costs several provider calls.
pub const DEFAULT_MAX_CONCURRENCY: usize = 4;

/// The response-body error code providers use for a prompt that does not fit.
/// The only string classification in the ladder, and deliberately the only one.
const CONTEXT_LENGTH_CODE: &str = "context_length_exceeded";

/// The fixed classification string a context-length refusal carries, in place of
/// the generic one. Actionable where the generic one is not — "shorten it" is a
/// different fix from "the request was malformed" — and, being a constant, it
/// carries no part of the body or message that selected it.
const CONTEXT_LENGTH_MESSAGE: &str =
    "the prompt does not fit the model's context window; send less text per call";

/// Prose a provider with no error code uses for the same condition. The
/// documented fallback: matched case-insensitively against the message only,
/// never against the body.
const CONTEXT_LENGTH_PROSE: [&str; 3] = [
    "prompt is too long",
    "maximum context length",
    "context window",
];

/// One model dispatch, as the provider SDK reports it.
///
/// The seam this crate's tests fake. It carries the SDK's shape rather than
/// this runtime's taxonomy precisely so the classification below is the thing
/// under test: a fake that already spoke [`LlmOutcome`] would test nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResponse {
    /// The provider's raw stop reason.
    pub stop_reason: StopReason,
    /// The completion text. `None` when the model produced none at all —
    /// distinct from `Some("")`, which is a model that produced an empty one.
    pub text: Option<String>,
    pub usage: ProviderUsage,
}

/// The provider's own stop reason, in its own vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// A natural stop — the only one that is `ok`.
    Stop,
    /// Hit the output cap mid-answer.
    Length,
    /// The safety filter stopped or redacted the completion.
    ContentFilter,
    /// The provider reported an error as a stop reason rather than throwing.
    Error,
    /// A stop reason this runtime does not classify. The raw spelling travels
    /// to the guest for diagnosis rather than widening the closed set.
    Other(String),
}

impl StopReason {
    /// The raw spelling, for [`LlmFailure::finish_reason`].
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ContentFilter => "content-filter",
            Self::Error => "error",
            Self::Other(raw) => raw,
        }
    }

    /// Step 3. The mapping is the whole of the `ok` rule: it keys off a natural stop,
    /// not off "nothing threw".
    fn failure_reason(&self) -> Option<FailureReason> {
        match self {
            Self::Stop => None,
            Self::Length => Some(FailureReason::Truncated),
            Self::ContentFilter => Some(FailureReason::ContentFiltered),
            Self::Error => Some(FailureReason::Incomplete),
            Self::Other(_) => Some(FailureReason::Incomplete),
        }
    }
}

/// Usage as the provider reported it, before the finite-number guard.
///
/// `f64` rather than `u64` because that is the shape the guard exists for: the
/// SDK's usage fields are JS numbers, so a provider can report a non-finite one,
/// and `NaN` must reach [`LlmOutcome`] as `None` rather than as `0`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProviderUsage {
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
}

impl Eq for ProviderUsage {}

impl ProviderUsage {
    pub fn reported(input_tokens: f64, output_tokens: f64) -> Self {
        Self {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
        }
    }

    /// Step 6. A count is usable only if it is present, finite, non-negative,
    /// and integral-in-range. Everything else is indeterminate — which is not
    /// the same as free, so it becomes `None` and the reconciler holds a
    /// conservative reserve for it.
    fn resolved(self) -> (Option<u64>, Option<u64>) {
        (
            finite_count(self.input_tokens),
            finite_count(self.output_tokens),
        )
    }
}

fn finite_count(value: Option<f64>) -> Option<u64> {
    let value = value?;
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 {
        return None;
    }
    Some(value as u64)
}

/// A dispatch that threw, as the provider SDK reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailure {
    /// The retry wrapper. Step 1 unwraps `last_error` before anything else:
    /// at the default retry count this is the *normal* shape of a persistent
    /// 429, and a classifier that reads the wrapper sees no status at all.
    Retry { last_error: Box<ProviderFailure> },
    /// The request was aborted on this side. A bare exception matching no SDK
    /// error class, which also short-circuits retry — so it is classified before
    /// the structural step, where it would otherwise read as a transport death.
    Abort,
    /// The provider answered with an HTTP error.
    ApiCall {
        /// Absent when the connection died before any response — which is
        /// exactly what distinguishes a transport failure from a provider that
        /// answered with an error.
        status: Option<u16>,
        /// The provider's error message. Read for step 5's prose fallback and
        /// then **dropped**: no part of it reaches the guest.
        message: String,
        /// The raw response body. Read for step 5's error code and then
        /// **dropped**: it carries request echoes and account identifiers.
        response_body: Option<String>,
        /// A `retry-after`, in seconds, when the provider sent one in the
        /// delta-seconds form.
        retry_after_secs: Option<u64>,
        /// Whether a `retry-after` header was present at all.
        ///
        /// Separate from `retry_after_secs` because the header also has an
        /// HTTP-date form, which carries the same "come back later" meaning
        /// while parsing to `None` as seconds. Keying `retryable` off the
        /// seconds alone reports a date-form 429 as non-retryable — the
        /// taxonomy's word for "the same request will keep failing" — and a
        /// guest retry loop would abandon a request that was going to succeed.
        retry_after_present: bool,
    },
    /// The structured-output path could not produce an object.
    ///
    /// `text` is raw model output by definition. It is carried here because the
    /// SDK carries it — and dropped at the taxonomy boundary, never reaching an
    /// [`LlmFailure`].
    NoObjectGenerated {
        text: Option<String>,
        usage: ProviderUsage,
        /// The stop reason the object path *did* observe, when it observed one.
        /// The object path never consults it itself, so a `content-filter` stop
        /// arrives here as an ordinary "no object" — and must not be flattened
        /// into `invalid-output`.
        stop_reason: Option<StopReason>,
    },
    /// The credential was rejected, or none was resolvable.
    Unauthorized,
    /// The connection failed with no status ever observed. `detail` is a fixed
    /// classification string — "connection reset", "dns lookup failed" — never a
    /// body and never an echo.
    Transport { detail: String },
}

/// One model dispatch. The seam the provider SDK sits behind, so classification
/// can be tested against hand-rolled fakes with zero live calls.
pub trait ModelDispatch: Send + Sync {
    fn dispatch<'a>(
        &'a self,
        request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>>;

    /// Check the provider row's credential before any element is dispatched.
    ///
    /// A credential that cannot resolve is a property of the configuration, not
    /// of any one prompt: without this the fan-out reports the same
    /// misconfiguration once per element, so a 128-prompt batch burns 128 slots
    /// to say the operator's key is missing, and the guest sees N per-element
    /// failures where the taxonomy has a dispatch-level variant meaning exactly
    /// this ([`LlmCallError::Unauthorized`]).
    ///
    /// Defaulted to `Ok(())` so an implementor with nothing to check — or no
    /// cheap way to check it — is unaffected.
    fn preflight<'a>(
        &'a self,
        _provider: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProviderFailure>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// What one element of a dispatch asks for.
#[derive(Debug, Clone, Copy)]
pub struct ModelRequest<'a> {
    /// The model as declared in the blueprint.
    pub model: &'a str,
    /// The provider row that model routes through.
    pub provider: &'a str,
    pub prompt: &'a str,
    /// The inlined JSON Schema, when the call is typed.
    pub schema_json: Option<&'a str>,
    /// Sent as the request's output cap, so the budget reservation is an actual
    /// upper bound rather than an estimate.
    pub output_cap: Option<u64>,
}

/// The outbound model provider bound to one blueprint.
pub struct BlueprintLlmProvider {
    blueprint: Arc<Blueprint>,
    dispatch: Arc<dyn ModelDispatch>,
    max_concurrency: usize,
}

impl BlueprintLlmProvider {
    pub fn new(blueprint: Arc<Blueprint>, dispatch: Arc<dyn ModelDispatch>) -> Self {
        Self {
            blueprint,
            dispatch,
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
        }
    }

    /// Override the fan-out bound. Zero is meaningless and would deadlock the
    /// semaphore, so it clamps to one.
    #[must_use]
    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.max_concurrency = max_concurrency.max(1);
        self
    }

    /// Declaration is authoritative: a model the blueprint does not declare is
    /// refused here, naming the block that declares one.
    fn resolve<'a>(&'a self, model: &str) -> Result<(&'a str, Option<u64>), LlmCallError> {
        let declared =
            self.blueprint
                .llm
                .models
                .get(model)
                .ok_or_else(|| LlmCallError::UnknownModel {
                    model: model.to_string(),
                    available: self.blueprint.llm.models.keys().cloned().collect(),
                })?;
        Ok((declared.provider.as_str(), declared.output_reserve))
    }
}

impl LlmProvider for BlueprintLlmProvider {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        schema_json: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>> {
        Box::pin(async move {
            let (provider, output_cap) = self.resolve(model)?;

            // A credential that cannot resolve belongs to the configuration,
            // not to any element, so it is refused once here rather than N
            // times inside the fan-out. This is the dispatch-level arm the
            // taxonomy reserves for it: nothing was dispatched, so no sibling
            // success is discarded by returning `Err` (R3 holds).
            if let Err(ProviderFailure::Unauthorized) = self.dispatch.preflight(provider).await {
                return Err(LlmCallError::Unauthorized {
                    model: model.to_string(),
                });
            }

            let limit = Arc::new(Semaphore::new(self.max_concurrency));

            // Positional ordering is contractual, and completion order is not
            // input order under varied per-element latency — so each element
            // carries its index and the results are placed, not pushed.
            let mut running: FuturesUnordered<_> = prompts
                .iter()
                .enumerate()
                .map(|(index, prompt)| {
                    let limit = Arc::clone(&limit);
                    async move {
                        let _permit = limit
                            .acquire()
                            .await
                            .expect("the semaphore outlives every permit it issues");
                        let request = ModelRequest {
                            model,
                            provider,
                            prompt,
                            schema_json,
                            output_cap,
                        };
                        (index, classify(self.dispatch.dispatch(request).await))
                    }
                })
                .collect();

            let mut outcomes: Vec<Option<LlmOutcome>> = vec![None; prompts.len()];
            while let Some((index, outcome)) = running.next().await {
                outcomes[index] = Some(outcome);
            }
            Ok(outcomes
                .into_iter()
                .map(|outcome| outcome.expect("every element reported exactly once"))
                .collect())
        })
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>> {
        Box::pin(async move {
            Ok(self
                .blueprint
                .llm
                .models
                .iter()
                .map(|(name, declared)| LlmModel {
                    name: name.clone(),
                    // Absent stays absent: a model nobody described reports
                    // `null`, never an invented string, and one nobody sized
                    // reports `null`, never zero.
                    description: declared.description.clone(),
                    context_window: declared.context_window,
                })
                .collect())
        })
    }

    /// Answered from the blueprint this impl already holds, so the reservation
    /// costs no round trip. Returning the declared value here is what makes the
    /// pre-dispatch reservation an upper bound: the same cap is sent as
    /// the request's output limit, so the provider cannot bill past what was
    /// reserved.
    fn output_reserve(&self, model: &str) -> Option<u64> {
        self.blueprint.llm.models.get(model)?.output_reserve
    }
}

/// The classification ladder. Steps are numbered against the module doc, and the
/// order is the contract.
fn classify(result: Result<ProviderResponse, ProviderFailure>) -> LlmOutcome {
    match result {
        Ok(response) => classify_response(response),
        Err(failure) => classify_failure(failure),
    }
}

/// Step 3 on the resolved path, plus step 6.
///
/// A resolved dispatch is *not* a success by itself: the text path resolves for
/// every stop reason, including `error`, and never inspects the stop reason to
/// raise. Eight of the nine outcomes come back here with a `text` a guest would
/// read as an answer.
fn classify_response(response: ProviderResponse) -> LlmOutcome {
    let (input_tokens, output_tokens) = response.usage.resolved();
    let outcome = match response.stop_reason.failure_reason() {
        None => LlmOutcome::success(response.text.unwrap_or_default()),
        Some(reason) => LlmOutcome::failed(
            LlmFailure::new(reason, reason.default_message())
                .with_finish_reason(response.stop_reason.as_str()),
            response.text,
        ),
    };
    outcome.with_usage(input_tokens, output_tokens)
}

fn classify_failure(failure: ProviderFailure) -> LlmOutcome {
    // Step 1: unwrap before anything else. Nested wrappers unwrap to the
    // innermost, since a wrapper's last error may itself be one.
    let failure = unwrap_retry(failure);

    match failure {
        // Step 1 unwrapped everything; a wrapper cannot survive to here.
        ProviderFailure::Retry { .. } => unreachable!("step 1 unwraps every retry wrapper"),

        // Step 2: before the structural step, where an abort would read as a
        // transport death — the cause is on this side, not the wire.
        ProviderFailure::Abort => failed(FailureReason::Cancelled, None::<String>),

        ProviderFailure::Unauthorized => failed(FailureReason::RequestRejected, None::<String>),

        ProviderFailure::Transport { .. } => failed(FailureReason::Transport, None::<String>),

        // Step 3 on the structured-output path. The object path never consults
        // the stop reason itself, so a `content-filter` stop arrives as an
        // ordinary "no object" and would flatten into `invalid-output` — the
        // asymmetry this arm exists to undo.
        //
        // `text` is raw model output and is dropped here, at the taxonomy
        // boundary, not carried onto the failure.
        ProviderFailure::NoObjectGenerated {
            text: _,
            usage,
            stop_reason,
        } => {
            let reason = stop_reason
                .as_ref()
                .and_then(StopReason::failure_reason)
                .unwrap_or(FailureReason::InvalidOutput);
            let mut error = LlmFailure::new(reason, reason.default_message());
            if let Some(stop_reason) = &stop_reason {
                error = error.with_finish_reason(stop_reason.as_str());
            }
            let (input_tokens, output_tokens) = usage.resolved();
            LlmOutcome::failed(error, None::<String>).with_usage(input_tokens, output_tokens)
        }

        ProviderFailure::ApiCall {
            status,
            message,
            response_body,
            retry_after_secs: _,
            retry_after_present,
        } => classify_api_call(
            status,
            &message,
            response_body.as_deref(),
            retry_after_present,
        ),
    }
}

/// Step 1. Loops rather than recursing once, because a wrapper's last error may
/// itself be a wrapper.
fn unwrap_retry(mut failure: ProviderFailure) -> ProviderFailure {
    while let ProviderFailure::Retry { last_error } = failure {
        failure = *last_error;
    }
    failure
}

/// Steps 4 and 5.
///
/// Neither `message` nor `body` is carried out of this function. They are read
/// to classify and then dropped: what survives is the status, the retryability,
/// and a fixed classification string.
fn classify_api_call(
    status: Option<u16>,
    message: &str,
    body: Option<&str>,
    retry_after_present: bool,
) -> LlmOutcome {
    // Step 4: structural first, because it is the part that does not depend on
    // provider prose.
    let Some(status) = status else {
        // No status was ever observed, so the connection died rather than the
        // provider answering.
        return failed(FailureReason::Transport, None::<String>);
    };

    let reason = match status {
        429 => FailureReason::RateLimited,
        500..=599 => FailureReason::ProviderUnavailable,
        // Everything else is a refusal of the request as sent — a prompt that
        // does not fit, a rejected credential, and a malformed request share an
        // arm because they share a shape: retrying the same bytes cannot help.
        _ => FailureReason::RequestRejected,
    };

    // Step 5. The closed set has no context-length member and must not grow one
    // — a guest branches on `reason`, and a prompt that does not fit is still a
    // request the provider refused. But "shorten the prompt" is a different fix
    // from "the request was malformed", and `default_message` is explicitly the
    // string to use *when nothing more specific is safe to say*. This is: a
    // second fixed constant, chosen by the classification, carrying no part of
    // the body or the message that selected it.
    let message =
        if reason == FailureReason::RequestRejected && is_context_length_exceeded(message, body) {
            CONTEXT_LENGTH_MESSAGE
        } else {
            reason.default_message()
        };

    let mut failure = LlmFailure::new(reason, message).with_status(status);
    // A 429 carrying a retry-after is retryable; one on an exhausted quota is
    // the provider saying the same request will keep failing. Keyed on the
    // header's presence, not on whether its value parsed as seconds — the
    // HTTP-date form says "come back later" just as plainly.
    if reason == FailureReason::RateLimited {
        failure = failure.retryable(retry_after_present);
    }
    LlmOutcome::failed(failure, None::<String>)
}

/// Step 5. The response body's error **code** first — a provider that emits one
/// is unambiguous — and prose as the documented fallback for the provider that
/// emits none.
///
/// Nothing matched here escapes: the return is a bool.
fn is_context_length_exceeded(message: &str, body: Option<&str>) -> bool {
    if body.is_some_and(|body| body.contains(CONTEXT_LENGTH_CODE)) {
        return true;
    }
    let message = message.to_ascii_lowercase();
    CONTEXT_LENGTH_PROSE
        .iter()
        .any(|prose| message.contains(prose))
}

fn failed(reason: FailureReason, text: Option<impl Into<String>>) -> LlmOutcome {
    LlmOutcome::failed(LlmFailure::new(reason, reason.default_message()), text)
}

#[cfg(test)]
mod tests;
