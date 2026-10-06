//! The recorded-world embedding provider.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::runtime::{
    EmbeddingBatch, EmbeddingError, EmbeddingFailureReason, EmbeddingModel, EmbeddingProvider,
    EmbeddingTokenBudget, Purpose,
};

use super::cassette::Cassette;

/// An [`EmbeddingProvider`] with no recording to answer from: a call log keeps an embed
/// call's texts and metadata but never the vectors, so every call is a miss and stops the
/// run, unless the run may go live and `declared` answers it.
///
/// `models`, `max_input_bytes` and `estimate_tokens` are answered by `declared`, the
/// provider the run would use live: they read the blueprint's declarations and do no I/O.
pub struct RecordedEmbeddingProvider {
    cassette: Arc<Cassette>,
    declared: Arc<dyn EmbeddingProvider>,
    live: bool,
}

impl RecordedEmbeddingProvider {
    pub fn new(cassette: Arc<Cassette>, declared: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            cassette,
            declared,
            live: false,
        }
    }

    /// Sends a call to `declared` instead of stopping the run.
    #[must_use]
    pub fn with_live(mut self) -> Self {
        self.live = true;
        self
    }
}

impl EmbeddingProvider for RecordedEmbeddingProvider {
    fn embed<'a>(
        &'a self,
        alias: &'a str,
        texts: &'a [String],
        purpose: Purpose,
        budget: &'a EmbeddingTokenBudget,
    ) -> Pin<Box<dyn Future<Output = Result<EmbeddingBatch, EmbeddingError>> + Send + 'a>> {
        Box::pin(async move {
            let miss = self.cassette.embedding_miss(alias);
            if self.live {
                self.cassette.went_live(miss);
                return self.declared.embed(alias, texts, purpose, budget).await;
            }
            self.cassette.stop(miss).await;
            Err(EmbeddingError::Provider {
                alias: alias.to_owned(),
                reason: EmbeddingFailureReason::Transport,
                settlements: Vec::new(),
            })
        })
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<EmbeddingModel>, EmbeddingError>> + Send + 'a>>
    {
        self.declared.models()
    }

    fn max_input_bytes(&self, alias: &str) -> Option<u64> {
        self.declared.max_input_bytes(alias)
    }

    fn estimate_tokens(&self, alias: &str, texts: &[String]) -> u64 {
        self.declared.estimate_tokens(alias, texts)
    }
}
