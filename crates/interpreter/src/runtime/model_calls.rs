//! Per-run model usage an embedder reads after guest code finishes.
//!
//! `submilli:llm` and `submilli:embedding` hand token counts back to the program.
//! The embedder that has to price the run does not see that value. It owns the
//! store, so each model call appends a record here once its outcome is in hand.
//!
//! The record is appended after `effects::execute` returns, from the outcome it
//! returned, whether that outcome was dispatched live or restored from history.
//! A durable run that resumes in a new attempt therefore lists every call of the
//! whole run, not only the calls this attempt sent. A call whose attempt ended
//! before its outcome was recorded is absent: its live retry is the one listed.

use super::call_log::ModelUsage;

/// One model call of the run, in call order.
///
/// `model` is the alias the program passed, not a provider id and not a pinned
/// revision. A missing count is indeterminate, not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallUsage {
    pub model: String,
    pub usage: AttributedUsage,
}

/// What the provider reported for one dispatch.
///
/// Embeddings are their own variant so a missing completion count — embeddings
/// have none — is not summed as unknown output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributedUsage {
    Llm(ModelUsage),
    Embedding { input_tokens: Option<u64> },
}

impl ModelCallUsage {
    pub(crate) fn llm(model: impl Into<String>, usage: ModelUsage) -> Self {
        Self {
            model: model.into(),
            usage: AttributedUsage::Llm(usage),
        }
    }

    pub(crate) fn embedding(model: impl Into<String>, input_tokens: Option<u64>) -> Self {
        Self {
            model: model.into(),
            usage: AttributedUsage::Embedding { input_tokens },
        }
    }

    /// One entry per `(kind, model)`, in the order each group first appeared.
    ///
    /// A missing input or output count stays missing. It is not zero, and it is
    /// not added to a known count. A cache count is a part of the input, so a
    /// missing one adds nothing. An embedding call never joins an llm group of
    /// the same alias.
    pub fn summed_by_model(calls: &[Self]) -> Vec<Self> {
        let mut totals: Vec<Self> = Vec::new();
        for call in calls {
            if let Some(total) = totals.iter_mut().find(|total| same_group(total, call)) {
                add_same_kind(&mut total.usage, &call.usage);
            } else {
                totals.push(call.clone());
            }
        }
        totals
    }
}

fn same_group(left: &ModelCallUsage, right: &ModelCallUsage) -> bool {
    left.model == right.model
        && matches!(
            (&left.usage, &right.usage),
            (AttributedUsage::Llm(_), AttributedUsage::Llm(_))
                | (
                    AttributedUsage::Embedding { .. },
                    AttributedUsage::Embedding { .. }
                )
        )
}

/// `same_group` has already required the kinds to match. A mismatch leaves the
/// total unchanged rather than inventing a cross-kind sum.
fn add_same_kind(total: &mut AttributedUsage, next: &AttributedUsage) {
    match (total, next) {
        (AttributedUsage::Llm(total), AttributedUsage::Llm(next)) => {
            *total = ModelUsage {
                input_tokens: add_count(total.input_tokens, next.input_tokens),
                output_tokens: add_count(total.output_tokens, next.output_tokens),
                cache_read_tokens: add_part(total.cache_read_tokens, next.cache_read_tokens),
                cache_write_tokens: add_part(total.cache_write_tokens, next.cache_write_tokens),
            };
        }
        (
            AttributedUsage::Embedding { input_tokens },
            AttributedUsage::Embedding {
                input_tokens: next_input,
            },
        ) => *input_tokens = add_count(*input_tokens, *next_input),
        _ => {}
    }
}

/// Both counts must be known for the sum to be known.
fn add_count(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    Some(left?.saturating_add(right?))
}

/// A part of a count: absent on both sides is absent, otherwise a missing side
/// adds nothing.
pub(crate) fn add_part(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0).saturating_add(right.unwrap_or(0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llm(model: &str, input: Option<u64>, output: Option<u64>) -> ModelCallUsage {
        ModelCallUsage::llm(
            model,
            ModelUsage {
                input_tokens: input,
                output_tokens: output,
                ..ModelUsage::default()
            },
        )
    }

    #[test]
    fn summed_by_model_adds_a_model_and_keeps_a_missing_count() {
        let calls = [
            llm("haiku", Some(10), Some(1)),
            llm("sonnet", Some(3), Some(4)),
            llm("haiku", None, Some(2)),
        ];
        assert_eq!(
            ModelCallUsage::summed_by_model(&calls),
            vec![llm("haiku", None, Some(3)), llm("sonnet", Some(3), Some(4)),]
        );
    }

    #[test]
    fn summed_by_model_adds_cache_parts_where_reported() {
        let cached = |read, write| {
            ModelCallUsage::llm(
                "sonnet",
                ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(1),
                    cache_read_tokens: read,
                    cache_write_tokens: write,
                },
            )
        };
        let summed = ModelCallUsage::summed_by_model(&[cached(Some(80), None), cached(None, None)]);
        assert_eq!(
            summed,
            vec![ModelCallUsage::llm(
                "sonnet",
                ModelUsage {
                    input_tokens: Some(200),
                    output_tokens: Some(2),
                    cache_read_tokens: Some(80),
                    cache_write_tokens: None,
                },
            )]
        );
    }

    #[test]
    fn summed_by_model_does_not_merge_llm_and_embedding() {
        let calls = [
            llm("haiku", Some(10), Some(1)),
            ModelCallUsage::embedding("haiku", Some(7)),
            ModelCallUsage::embedding("haiku", None),
        ];
        assert_eq!(
            ModelCallUsage::summed_by_model(&calls),
            vec![
                llm("haiku", Some(10), Some(1)),
                ModelCallUsage::embedding("haiku", None),
            ]
        );
    }
}
