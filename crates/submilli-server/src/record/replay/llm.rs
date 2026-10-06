//! The recorded-world model provider.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::runtime::{
    BodyCopy, FailureReason, LlmCallError, LlmFailure, LlmModel, LlmOutcome, LlmProvider,
};
use interpreter::stdlib::llm::{OPS, request_digest};
use serde_json::Value;

use super::cassette::{Cassette, Entry, Kind, Unusable, llm_key};

/// An [`LlmProvider`] that answers from a recorded run: the next unused recording of the
/// same model, prompts and schema, served whole, with each outcome's own failure and token
/// counts.
///
/// `models` and `output_reserve` are answered by `declared`, the provider the run would
/// use live: they read the blueprint's declarations and do no I/O.
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
                Ok(outcomes) => Ok(outcomes),
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

fn answer(entry: &Entry, prompts: usize) -> Result<Vec<LlmOutcome>, Unusable> {
    let response = entry
        .response
        .as_ref()
        .ok_or_else(|| Unusable::incomplete("the recorded call never finished"))?;
    if response.truncated {
        return Err(Unusable::incomplete(
            "the recorded response was cut by the recorder's caps",
        ));
    }
    let unreadable = || Unusable::incomplete("the recorded response is unreadable");
    let Some(BodyCopy::Text(texts)) = &response.body else {
        return Err(Unusable::incomplete(
            "the recorder kept the digest of the response, not its text",
        ));
    };
    let texts: Vec<Option<String>> = serde_json::from_str(texts).map_err(|_| unreadable())?;
    let meta = &response.meta;
    let array = |name: &str| meta[name].as_array().filter(|items| items.len() == prompts);
    let (Some(ok), Some(failures), Some(usage)) = (array("ok"), array("failures"), array("usage"))
    else {
        return Err(unreadable());
    };
    if texts.len() != prompts {
        return Err(unreadable());
    }
    let tokens = |value: &Value| value.as_u64();
    (0..prompts)
        .map(|at| {
            let ok = ok[at].as_bool().ok_or_else(unreadable)?;
            let text = texts[at].clone();
            let (input_tokens, output_tokens) = (
                tokens(&usage[at]["input_tokens"]),
                tokens(&usage[at]["output_tokens"]),
            );
            if ok {
                if text.is_none() {
                    return Err(unreadable());
                }
                return Ok(LlmOutcome {
                    ok,
                    text,
                    failure: None,
                    input_tokens,
                    output_tokens,
                });
            }
            Ok(LlmOutcome {
                ok,
                text,
                failure: Some(failure(&failures[at])?),
                input_tokens,
                output_tokens,
            })
        })
        .collect()
}

/// An outcome's failure, from the record the call log keeps.
fn failure(record: &Value) -> Result<LlmFailure, Unusable> {
    let kind = record
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Unusable::failure("a recorded failure kept no kind"))?;
    let reason = FailureReason::ALL
        .into_iter()
        .find(|reason| reason.as_str() == kind)
        .ok_or_else(|| Unusable::failure(format!("a recorded `{kind}` failure is unknown")))?;
    Ok(LlmFailure {
        reason,
        message: record["message"].as_str().unwrap_or_default().to_owned(),
        retryable: record["retryable"]
            .as_bool()
            .unwrap_or_else(|| reason.retryable_by_default()),
        status: record["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok()),
        finish_reason: record["finish_reason"].as_str().map(str::to_owned),
    })
}
