//! Per-run model usage an embedder reads after guest code finishes.
//!
//! `submilli:llm` and `submilli:embedding` hand token counts back to the program.
//! The embedder that has to price the run does not see that value. It owns the
//! store, so the live dispatch appends a record here. Replay restores a recorded
//! outcome without running the provider action, and therefore must not append:
//! recording after `effects::execute` returns would bill the same call again.

use super::call_log::ModelUsage;

/// One live model dispatch in this attempt, in call order.
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
    Llm {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    Embedding {
        input_tokens: Option<u64>,
    },
}

impl ModelCallUsage {
    pub(crate) fn llm(model: impl Into<String>, usage: ModelUsage) -> Self {
        Self {
            model: model.into(),
            usage: AttributedUsage::Llm {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            },
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
    /// A missing count stays missing. It is not zero, and it is not added to a
    /// known count. An embedding call never joins an llm group of the same alias.
    pub fn summed_by_model(calls: &[Self]) -> Vec<Self> {
        let mut totals = Vec::new();
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
    left.model == right.model && same_kind(&left.usage, &right.usage)
}

fn same_kind(left: &AttributedUsage, right: &AttributedUsage) -> bool {
    matches!(
        (left, right),
        (AttributedUsage::Llm { .. }, AttributedUsage::Llm { .. })
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
        (
            AttributedUsage::Llm {
                input_tokens,
                output_tokens,
            },
            AttributedUsage::Llm {
                input_tokens: next_input,
                output_tokens: next_output,
            },
        ) => {
            *input_tokens = add_count(*input_tokens, *next_input);
            *output_tokens = add_count(*output_tokens, *next_output);
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

fn add_count(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    Some(left?.saturating_add(right?))
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
