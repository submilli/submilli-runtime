//! The recorded-world model provider.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::runtime::{
    FailureReason, LlmCallError, LlmFailure, LlmModel, LlmOutcome, LlmProvider,
};
use interpreter::stdlib::llm::{OPS, request_digest};
use serde_json::Value;

use super::cassette::{Cassette, Entry, Kind, Unusable, decoded_body, llm_key};

/// An [`LlmProvider`] that answers from a recorded run: the next unused recording of the
/// same model, prompts and schema, served whole, with each outcome's own failure and token
/// counts.
///
/// `models` and `output_reserve` are answered by `declared`, the provider the run would
/// use live: they read the blueprint's declarations and do no I/O. A model it does not
/// declare is refused as it refuses it live, whatever was recorded.
///
/// The recorded token counts come back as usage, so a budget the run holds is charged as
/// it was when the calls were made. A test run that must not spend tokens runs without one.
pub struct RecordedLlmProvider {
    cassette: Arc<Cassette>,
    declared: Arc<dyn LlmProvider>,
    live: bool,
}

impl RecordedLlmProvider {
    pub fn new(cassette: Arc<Cassette>, declared: Arc<dyn LlmProvider>) -> Self {
        Self {
            cassette,
            declared,
            live: false,
        }
    }

    /// Sends a call the recording cannot answer to `declared` instead of stopping the run.
    #[must_use]
    pub fn with_live(mut self) -> Self {
        self.live = true;
        self
    }
}

impl LlmProvider for RecordedLlmProvider {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        schema_json: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>> {
        Box::pin(async move {
            let declared = self.declared.models().await?;
            if !declared.iter().any(|declared| declared.name == model) {
                return Err(LlmCallError::UnknownModel {
                    model: model.to_owned(),
                    available: declared.into_iter().map(|declared| declared.name).collect(),
                });
            }
            // The provider is not told whether the program called `call` or `batch`, and
            // answers both alike.
            let accept: Vec<String> = OPS
                .iter()
                .map(|op| request_digest(op, model, prompts, schema_json))
                .collect();
            match self
                .cassette
                .serve(Kind::Llm, &llm_key(model), &accept, |entry| {
                    answer(entry, prompts.len())
                }) {
                Ok(answered) => answered,
                Err(miss) if self.live => {
                    self.cassette.went_live(miss);
                    self.declared.call(model, prompts, schema_json).await
                }
                Err(miss) => {
                    self.cassette.stop(miss).await;
                    Err(LlmCallError::Transport {
                        model: model.to_owned(),
                        detail: "no recorded response".into(),
                    })
                }
            }
        })
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>> {
        self.declared.models()
    }

    fn output_reserve(&self, model: &str) -> Option<u64> {
        self.declared.output_reserve(model)
    }
}

/// The recorded answer: the outcomes of each prompt, or the error the whole call failed with.
fn answer(
    entry: &Entry,
    prompts: usize,
) -> Result<Result<Vec<LlmOutcome>, LlmCallError>, Unusable> {
    let response = entry.finished_response()?;
    let meta = &response.meta;
    if let Some(recorded) = meta.get("call_error") {
        return call_error(recorded).map(Err);
    }
    let unreadable = || Unusable::incomplete("the recorded response is unreadable");
    let texts: Vec<Option<String>> =
        serde_json::from_slice(&decoded_body(response, "text")?).map_err(|_| unreadable())?;
    let array = |name: &str| meta[name].as_array().filter(|items| items.len() == prompts);
    let (Some(ok), Some(failures), Some(usage)) = (array("ok"), array("failures"), array("usage"))
    else {
        return Err(unreadable());
    };
    if texts.len() != prompts {
        return Err(unreadable());
    }
    let tokens = |value: &Value| value.as_u64();
    let outcomes: Result<Vec<_>, Unusable> = (0..prompts)
        .map(|at| {
            let ok = ok[at].as_bool().ok_or_else(unreadable)?;
            let text = texts[at].clone();
            let (input_tokens, output_tokens) = (
                tokens(&usage[at]["input_tokens"]),
                tokens(&usage[at]["output_tokens"]),
            );
            if ok && text.is_none() {
                return Err(unreadable());
            }
            let failure = if ok {
                None
            } else {
                Some(failure(&failures[at])?)
            };
            Ok(LlmOutcome {
                ok,
                text,
                failure,
                input_tokens,
                output_tokens,
            })
        })
        .collect();
    outcomes.map(Ok)
}

/// A whole-call failure, from the record the call log keeps: only an answer from outside
/// is raised again. Every `LlmCallError` kind:
///
/// | kind                     | class                                                       |
/// |--------------------------|-------------------------------------------------------------|
/// | `transport`              | outside: the wire died before any element ran; served       |
/// | `unauthorized`           | local: credential unresolved or rejected; today's key decides; miss |
/// | `not-configured`         | local: no provider wired; miss                              |
/// | `unknown-model`          | local: today's declarations; miss                           |
/// | `budget-exceeded`        | local: today's token budget; miss                           |
/// | `prompt-bounds-exceeded` | local: today's prompt bounds; miss                          |
///
/// A kind not listed, or none, is a miss too.
fn call_error(record: &Value) -> Result<LlmCallError, Unusable> {
    let text = |name: &str| record[name].as_str().map(str::to_owned);
    let model =
        text("model").ok_or_else(|| Unusable::failure("a recorded failure kept no model"))?;
    match record["kind"].as_str() {
        Some("transport") => Ok(LlmCallError::Transport {
            model,
            detail: text("detail").unwrap_or_default(),
        }),
        other => Err(Unusable::decided_today(other)),
    }
}

/// An outcome's failure, from the record the call log keeps: only an answer the model's
/// provider gave is raised again. Every `FailureReason`, and the `local` kind the call log
/// gives a refusal by this host's configuration or network policy (a provider the
/// blueprint does not describe, a blocked endpoint, an unresolved credential):
///
/// | kind                                                    | class                         |
/// |---------------------------------------------------------|-------------------------------|
/// | truncated, content-filtered, invalid-output             | outside: what the model returned; served (see below) |
/// | rate-limited, provider-unavailable, incomplete          | outside: the provider's answer; served |
/// | transport                                               | outside: the wire failed; served |
/// | request-rejected                                        | outside: the provider's refusal; served, but a 401 or 403 is the credential it ran with, so a miss |
/// | cancelled                                               | local: this run's own cancel; miss |
/// | local, or a kind not listed                             | local: miss                   |
///
/// The output cap and structured-output support are not in the request digest, so a served
/// `truncated` or `invalid-output` reflects the source run's settings, not today's: an accepted
/// limit of the design.
fn failure(record: &Value) -> Result<LlmFailure, Unusable> {
    let kind = record.get("kind").and_then(Value::as_str);
    let reason = kind
        .and_then(|kind| {
            FailureReason::ALL
                .into_iter()
                .find(|reason| reason.as_str() == kind)
        })
        .ok_or_else(|| Unusable::decided_today(kind))?;
    let status = record["status"]
        .as_u64()
        .and_then(|status| u16::try_from(status).ok());
    let outside = match reason {
        FailureReason::Truncated
        | FailureReason::ContentFiltered
        | FailureReason::InvalidOutput
        | FailureReason::RateLimited
        | FailureReason::ProviderUnavailable
        | FailureReason::Transport
        | FailureReason::Incomplete => true,
        FailureReason::RequestRejected => !matches!(status, Some(401 | 403)),
        FailureReason::Cancelled => false,
    };
    if !outside {
        return Err(Unusable::decided_today(kind));
    }
    Ok(LlmFailure {
        reason,
        message: record["message"].as_str().unwrap_or_default().to_owned(),
        retryable: record["retryable"]
            .as_bool()
            .unwrap_or_else(|| reason.retryable_by_default()),
        status,
        finish_reason: record["finish_reason"].as_str().map(str::to_owned),
        local: false,
    })
}
