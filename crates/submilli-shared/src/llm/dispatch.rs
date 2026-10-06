//! The real outbound [`ModelDispatch`]: one HTTP round trip per element.
//!
//! Mirrors [`crate::mcp::transport::StreamableHttpTransport`] — the same job on
//! the other capability. It holds the blueprint and the secret store, resolves
//! its own `${secrets.X}` credential through the same
//! [`interpolate`]/[`BlueprintSecretResolver`] path, and makes its own outbound
//! call. Nothing above it ever sees the key.
//!
//! **What this file does not do: classify.** [`super::provider`] owns the whole
//! ladder — retry unwrap, abort, stop reason, structural HTTP, context-length
//! body sniffing, the finite-usage guard. This file's contract is to produce a
//! [`ProviderResponse`] or a [`ProviderFailure`] that describes what actually
//! happened on the wire, and to let the ladder decide what it means. A dispatch
//! that pre-classified would give the ladder nothing left to classify, and its
//! empirically-verified ordering would silently stop being the thing under test.
//!
//! **No retry here.** The ladder already unwraps a retry wrapper and already
//! distinguishes a 429 with a `retry-after` (retryable) from one without
//! (exhausted quota), and it reports both to the guest as a `rate-limited`
//! outcome carrying `retryable`. A retry loop inside the dispatch would spend
//! real tokens against the operator's credential *before* the budget reconciler
//! ever sees the first attempt, multiply every element's cost by the retry count
//! under a fan-out bound chosen on the assumption that it would not, and hide
//! from the guest the very signal the outcome type exists to hand it. Retrying
//! is the caller's decision because the caller is the one whose budget pays for
//! it.
//!
//! **R13/KTD7 in this file specifically.** The key is written into a header and
//! never into a URL (Google's `?key=` form is refused for that reason), never
//! into a body, and never into an error. Every error constructed here carries a
//! fixed classification string; the one place a response body is attached is
//! [`wire::build_failure`], whose output goes straight to the ladder that reads
//! and drops it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use interpreter::stdlib::http::NetworkPolicy;
use serde_json::Value;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, LlmProviderDecl};

use crate::http_client::{self, TransportFailure};
use crate::secret_store::SecretStore;

use super::provider::{ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse};
use super::wire::{self, ProviderKind};

/// The outbound model dispatch bound to one blueprint.
///
/// Built per execute, like the MCP transport it mirrors, because it resolves
/// credentials from *this* blueprint and *this* session's harness bindings.
pub struct HttpModelDispatch {
    blueprint: Arc<Blueprint>,
    secret_store: Option<Arc<dyn SecretStore>>,
    harness_secrets: Arc<HarnessSecretBindings>,
    /// The server's outbound-address policy. A provider `base_url` is the
    /// blueprint author's to write, so it is judged like any other outbound
    /// destination: by the resolver for host names, and by
    /// [`NetworkPolicy::check_literal_host`] for literal IPs.
    policy: Arc<NetworkPolicy>,
    client: reqwest::Client,
}

/// Failure to construct the outbound client before any model call is dispatched.
#[derive(Debug)]
pub struct HttpModelDispatchError {
    source: reqwest::Error,
}

impl std::fmt::Display for HttpModelDispatchError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("LLM dispatch initialization failed")
    }
}

impl std::error::Error for HttpModelDispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl From<reqwest::Error> for HttpModelDispatchError {
    fn from(source: reqwest::Error) -> Self {
        Self { source }
    }
}

impl HttpModelDispatch {
    pub fn new(
        blueprint: Arc<Blueprint>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
    ) -> Result<Self, HttpModelDispatchError> {
        let builder = policy.client_builder();
        Self::with_client_builder(blueprint, secret_store, policy, builder)
    }

    fn with_client_builder(
        blueprint: Arc<Blueprint>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
        builder: reqwest::ClientBuilder,
    ) -> Result<Self, HttpModelDispatchError> {
        // Refuse redirects: Anthropic and Google authenticate using custom
        // headers that reqwest otherwise forwards to a redirected destination.
        // Cookies are disabled, so elements can share this connection pool.
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

    /// Resolve the row's `${secrets.X}` into the actual key.
    ///
    /// A declared-but-unresolvable key is [`ProviderFailure::Unauthorized`],
    /// which carries no detail at all, so neither the secret's name nor the
    /// resolver's message escapes. See [`http_client::resolve_api_key`].
    async fn api_key(&self, decl: &LlmProviderDecl) -> Result<Option<String>, ProviderFailure> {
        http_client::resolve_api_key(
            decl.api_key.as_deref(),
            &self.blueprint,
            self.secret_store.clone(),
            Arc::clone(&self.harness_secrets),
        )
        .await
        .map_err(|_| ProviderFailure::Unauthorized)
    }

    async fn dispatch_once(
        &self,
        request: ModelRequest<'_>,
    ) -> Result<ProviderResponse, ProviderFailure> {
        let decl = self.blueprint.llm.providers.get(request.provider).ok_or(
            // The blueprint validator refuses a model naming an undeclared
            // provider, and `BlueprintLlmProvider::resolve` refuses an
            // undeclared model before reaching here, so this is unreachable
            // through the normal path — reported rather than panicked on
            // because a `Blueprint` can also be built in memory.
            ProviderFailure::Transport {
                detail: "provider is not declared".to_string(),
            },
        )?;

        let Some(kind) = ProviderKind::from_type(&decl.provider_type) else {
            return Err(ProviderFailure::Transport {
                detail: "provider type is not supported".to_string(),
            });
        };

        let Some(base_url) = decl.base_url.as_deref().or_else(|| kind.default_base_url()) else {
            return Err(ProviderFailure::Transport {
                detail: "provider declares no endpoint".to_string(),
            });
        };

        // The opt-out is honored by sending no schema at all, not by sending a
        // weaker one: `{"type":"json_object"}` would ask for shapeless JSON and
        // let an unvalidated answer wear a verified type. Dropping the schema
        // instead means the unconditional structural check on the typed path is
        // what refuses the response, loudly.
        let schema = request
            .schema_json
            .filter(|_| decl.supports_structured_outputs)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());

        let key = self.api_key(decl).await?;
        let wire_request = wire::build_request(
            kind,
            base_url,
            request.model,
            request.prompt,
            schema.as_ref(),
            request.output_cap,
            key.as_deref(),
        );

        self.policy
            .check_literal_host(&wire_request.url)
            .map_err(|detail| ProviderFailure::Transport { detail })?;
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
            .map_err(http_client::map_transport_error)?;

        let status = response.status().as_u16();
        let (retry_after_present, retry_after) = http_client::retry_after(response.headers());
        // The body is read on both arms: the success parser needs it, and the
        // failure arm needs it for the ladder's context-length classification.
        let body = http_client::read_bounded(response).await?;

        if (200..300).contains(&status) {
            wire::parse_response(kind, &body)
        } else {
            Err(wire::build_failure(
                status,
                &body,
                retry_after,
                retry_after_present,
            ))
        }
    }
}

impl ModelDispatch for HttpModelDispatch {
    fn dispatch<'a>(
        &'a self,
        request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        Box::pin(async move { self.dispatch_once(request).await })
    }

    /// Resolve the row's credential and discard it.
    ///
    /// Only the resolution can fail; the key itself is dropped immediately
    /// rather than cached, so this adds no lifetime to a secret. Each element
    /// resolves its own when it runs — this exists to answer "can it resolve at
    /// all" before the fan-out commits to N attempts.
    fn preflight<'a>(
        &'a self,
        provider: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProviderFailure>> + Send + 'a>> {
        Box::pin(async move {
            let Some(decl) = self.blueprint.llm.providers.get(provider) else {
                // An undeclared provider is caught by the caller's own
                // resolution against the blueprint, so there is nothing to
                // check and nothing to report here.
                return Ok(());
            };
            self.api_key(decl).await.map(|_| ())
        })
    }
}

impl From<TransportFailure> for ProviderFailure {
    fn from(failure: TransportFailure) -> Self {
        ProviderFailure::Transport {
            detail: failure.detail,
        }
    }
}

#[cfg(test)]
mod tests;
