//! The real outbound [`EmbeddingDispatch`]: one HTTP round trip per sub-batch.
//!
//! Mirrors [`crate::llm::dispatch::HttpModelDispatch`]. It holds the blueprint
//! and the secret store, resolves the provider's `${secrets.X}` key itself, and
//! makes its own outbound call; nothing above it sees the key.
//!
//! **Classification lives in [`super::wire`].** This file produces a
//! [`DispatchResponse`] or a [`DispatchFailure`] describing what happened on the
//! wire and leaves the meaning to the provider core.
//!
//! **No retry here.** A retry would spend tokens against the operator's
//! credential before the budget reconciler sees the first attempt.
//!
//! **No secret, URL, input text or body leaves this file.** The key travels in a
//! header only; every error is a fixed classification.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::stdlib::http::NetworkPolicy;
use submilli_blueprint::{Blueprint, HarnessSecretBindings};

use crate::http_client::{self, TransportFailure};
use crate::secret_store::SecretStore;

use super::wire;
use super::{
    DispatchFailure, DispatchResponse, EmbeddingDispatch, EmbeddingRequest, NotSentReason,
    Rejection, SentFailure,
};

/// The outbound embedding dispatch bound to one blueprint.
///
/// Built per execute, because it resolves credentials from *this* blueprint and
/// *this* session's harness bindings.
pub struct HttpEmbeddingDispatch {
    blueprint: Arc<Blueprint>,
    secret_store: Option<Arc<dyn SecretStore>>,
    harness_secrets: Arc<HarnessSecretBindings>,
    /// Judges a provider `base_url` like any other outbound destination: by the
    /// resolver for host names, and by [`NetworkPolicy::check_literal_host`] for
    /// literal IPs.
    policy: Arc<NetworkPolicy>,
    client: reqwest::Client,
}

/// Failure to construct the outbound client before any embedding call is dispatched.
#[derive(Debug)]
pub struct HttpEmbeddingDispatchError {
    source: reqwest::Error,
}

impl std::fmt::Display for HttpEmbeddingDispatchError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("embedding dispatch initialization failed")
    }
}

impl std::error::Error for HttpEmbeddingDispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl From<reqwest::Error> for HttpEmbeddingDispatchError {
    fn from(source: reqwest::Error) -> Self {
        Self { source }
    }
}

impl HttpEmbeddingDispatch {
    pub fn new(
        blueprint: Arc<Blueprint>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
    ) -> Result<Self, HttpEmbeddingDispatchError> {
        let builder = policy.client_builder();
        Self::with_client_builder(blueprint, secret_store, policy, builder)
    }

    fn with_client_builder(
        blueprint: Arc<Blueprint>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
        builder: reqwest::ClientBuilder,
    ) -> Result<Self, HttpEmbeddingDispatchError> {
        let client = http_client::configure_client(builder).build()?;
        Ok(Self {
            blueprint,
            secret_store,
            harness_secrets: Arc::new(HarnessSecretBindings::new()),
            client,
            policy,
        })
    }

    pub fn with_harness_secrets(mut self, secrets: Arc<HarnessSecretBindings>) -> Self {
        self.harness_secrets = secrets;
        self
    }

    /// Resolve a declared `${secrets.X}` into the key. `None` means the provider
    /// declares none (a Hugging Face dedicated endpoint). An unresolvable key is
    /// [`DispatchFailure::NotSent`], which carries only a fixed reason: neither the
    /// secret's name nor the resolver's message escapes.
    async fn api_key(&self, reference: Option<&str>) -> Result<Option<String>, DispatchFailure> {
        http_client::resolve_api_key(
            reference,
            &self.blueprint,
            self.secret_store.clone(),
            Arc::clone(&self.harness_secrets),
        )
        .await
        .map_err(|_| DispatchFailure::NotSent(NotSentReason::CredentialUnresolved))
    }

    async fn dispatch_once(
        &self,
        request: EmbeddingRequest<'_>,
    ) -> Result<DispatchResponse, DispatchFailure> {
        let key = self.api_key(request.api_key).await?;
        let wire_request = wire::build_request(&request, key.as_deref())
            // Nothing was sent and nothing is owed; a free rejection.
            .map_err(|_| DispatchFailure::Rejected(Rejection::Other))?;

        self.policy
            .check_literal_host(&wire_request.url)
            .map_err(|_| DispatchFailure::NotSent(NotSentReason::Blocked))?;
        let mut builder = self
            .client
            .post(&wire_request.url)
            .header("content-type", "application/json");
        for (name, value) in &wire_request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }

        let response = builder
            .json(&wire_request.body)
            .send()
            .await
            .map_err(send_failure)?;

        let status = response.status().as_u16();
        let (_, retry_after) = http_client::retry_after(response.headers());
        let body = http_client::read_bounded(response)
            .await
            .map_err(failed_from)?;

        if (200..300).contains(&status) {
            wire::parse_response(request.provider_type, &body)
        } else {
            Err(wire::classify_failure(
                request.provider_type,
                status,
                &body,
                retry_after,
                request.texts.len(),
            ))
        }
    }
}

/// A failure of `send()`. A request that could not be built, or never got a
/// connection (including a resolver the network policy refused), put no bytes on
/// the wire and is released; anything later may have been billed.
fn send_failure(err: reqwest::Error) -> DispatchFailure {
    let never_left = err.is_builder() || err.is_connect();
    let failure = http_client::map_transport_error(err);
    if failure
        .detail
        .contains(http_client::BLOCKED_BY_POLICY_DETAIL)
    {
        return DispatchFailure::NotSent(NotSentReason::Blocked);
    }
    if never_left {
        return DispatchFailure::NotSent(NotSentReason::Unreachable);
    }
    failed_from(failure)
}

/// A transport failure becomes a timeout only when the shared helper said so.
fn failed_from(failure: TransportFailure) -> DispatchFailure {
    let kind = if failure.detail == http_client::TIMEOUT_DETAIL {
        SentFailure::Timeout
    } else {
        SentFailure::Transport
    };
    DispatchFailure::Failed { kind, usage: None }
}

impl EmbeddingDispatch for HttpEmbeddingDispatch {
    fn dispatch<'a>(
        &'a self,
        request: EmbeddingRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>> {
        Box::pin(async move { self.dispatch_once(request).await })
    }

    /// Resolve the provider's credential and discard it, so a missing secret
    /// costs nothing. A provider declaring no key (Hugging Face dedicated)
    /// passes.
    fn preflight<'a>(
        &'a self,
        provider: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), DispatchFailure>> + Send + 'a>> {
        Box::pin(async move {
            let Some(decl) = self.blueprint.embedding.providers.get(provider) else {
                // An undeclared provider is refused by the caller's own alias
                // resolution; there is nothing to check here.
                return Ok(());
            };
            self.api_key(decl.api_key.as_deref()).await.map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests;
