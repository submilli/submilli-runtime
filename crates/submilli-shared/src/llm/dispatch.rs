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
use std::time::Duration;

use interpreter::stdlib::http::{NetworkPolicy, describe_error_chain};
use serde_json::Value;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, LlmProviderDecl, interpolate};

use crate::host::BlueprintSecretResolver;
use crate::secret_store::SecretStore;

use super::provider::{ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse};
use super::wire::{self, ProviderKind};

/// How long one dispatch may take end to end.
///
/// Generous by HTTP standards and deliberately so: a large reasoning request
/// legitimately runs for minutes, and a timeout tuned for an ordinary API would
/// turn a working call into a `transport` failure that costs the operator the
/// tokens anyway. It exists to stop a hung socket pinning a fan-out slot
/// forever, not to bound model latency.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// How long the TCP/TLS handshake may take, separately from the request as a
/// whole.
///
/// Establishing a connection is not where a model spends its time, so this can
/// be short where [`REQUEST_TIMEOUT`] cannot. Without it, an endpoint whose
/// address is routable-but-dead — a typo'd `base_url`, a decommissioned host —
/// holds a fan-out slot for the OS's own SYN-retry budget, which is over a
/// minute on most platforms, before the generous overall timeout even begins to
/// apply.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A `retry-after` beyond this is treated as absent. The header is advisory and
/// the value only feeds the `retryable` flag, so an absurd one should not make a
/// guest wait on a number no provider meant.
const MAX_RETRY_AFTER_SECS: u64 = 3600;

/// The most of one response this process will buffer.
///
/// A completion is text bounded by the model's own output cap, so this is far
/// above any legitimate answer; it exists because the peer on an
/// `openai-compatible` connection is operator-supplied and therefore not
/// necessarily one to trust to bound its own body.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

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

impl HttpModelDispatch {
    pub fn new(
        blueprint: Arc<Blueprint>,
        secret_store: Option<Arc<dyn SecretStore>>,
        policy: Arc<NetworkPolicy>,
    ) -> Self {
        Self {
            blueprint,
            secret_store,
            harness_secrets: Arc::new(HarnessSecretBindings::new()),
            // No cookie store is ever enabled, so the client carries no
            // cross-request state and one pool is safe to share across the
            // elements of a fan-out.
            client: policy
                .client_builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                // Redirects are refused outright, which is stricter than the
                // `submilli:http` client and deliberately so.
                //
                // reqwest's default follows up to ten hops and strips only
                // `Authorization` when the host changes. Two of the four kinds
                // authenticate with a *custom* header — Anthropic's `x-api-key`
                // and Google's `x-goog-api-key` — which survive the hop, so a
                // `307 Location: https://attacker/` from any endpoint would
                // hand over the operator's key and the prompt body. The
                // exposure is inverted from intuition: the first-party kinds
                // are the vulnerable ones, and `openai`/`openai-compatible`
                // are only incidentally safe for riding `authorization`.
                //
                // No provider's completions endpoint legitimately redirects, so
                // a redirect is refused rather than followed to a
                // same-host-only policy: a `3xx` here means the endpoint is not
                // what the operator declared, and that is worth surfacing as a
                // transport failure rather than quietly chasing.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("the reqwest client builds from static configuration"),
            policy,
        }
    }

    pub fn with_harness_secrets(mut self, secrets: Arc<HarnessSecretBindings>) -> Self {
        self.harness_secrets = secrets;
        self
    }

    /// Resolve the row's `${secrets.X}` into the actual key.
    ///
    /// A row that declares no `api_key` resolves to `None` rather than an error:
    /// a locally-hosted `openai-compatible` endpoint may genuinely need none,
    /// and inventing a credential requirement it does not have would refuse a
    /// working deployment. A declared-but-unresolvable key *is* an error, and it
    /// is [`ProviderFailure::Unauthorized`] — which carries no detail at all, so
    /// neither the secret's name nor the resolver's message escapes.
    async fn api_key(&self, decl: &LlmProviderDecl) -> Result<Option<String>, ProviderFailure> {
        let Some(reference) = &decl.api_key else {
            return Ok(None);
        };
        let resolver = BlueprintSecretResolver::with_harness(
            self.secret_store.clone(),
            Arc::clone(&self.harness_secrets),
        );
        match interpolate(reference, &self.blueprint, &resolver).await {
            Ok(key) if key.is_empty() => Err(ProviderFailure::Unauthorized),
            Ok(key) => Ok(Some(key)),
            Err(_) => Err(ProviderFailure::Unauthorized),
        }
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
            .map_err(map_transport_error)?;

        let status = response.status().as_u16();
        let (retry_after_present, retry_after) = retry_after(response.headers());
        // The body is read on both arms: the success parser needs it, and the
        // failure arm needs it for the ladder's context-length classification.
        let body = read_bounded(response).await?;

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

/// Read the body, refusing to buffer more than [`MAX_RESPONSE_BYTES`].
///
/// `openai-compatible` endpoints are operator-supplied, so the peer is not
/// necessarily one this process should trust to bound its own response. The
/// interpreter's outbound HTTP client caps bodies for the same reason; this is
/// the same guarantee on the same kind of connection, and it stops reading
/// rather than reading and then measuring.
///
/// The overflow is a transport failure carrying only a fixed string — no part of
/// the body it refused to finish reading.
async fn read_bounded(response: reqwest::Response) -> Result<String, ProviderFailure> {
    use futures::StreamExt as _;

    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_transport_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(ProviderFailure::Transport {
                detail: "response exceeded the size limit".to_string(),
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| ProviderFailure::Transport {
        detail: "response body was not valid UTF-8".to_string(),
    })
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

/// Map a transport-level reqwest failure.
///
/// `detail` is one of a fixed set of classification strings, never the error's
/// own message: reqwest renders the URL into its `Display`, and a
/// `openai-compatible` URL is operator-supplied. The strings are the vocabulary
/// [`ProviderFailure::Transport`] documents.
fn map_transport_error(err: reqwest::Error) -> ProviderFailure {
    // A resolver refusal surfaces as a connect error; the policy's reason sits at
    // the bottom of the chain and is what the operator needs to see.
    let chain = describe_error_chain(&err);
    if chain.contains("blocked by network policy") {
        return ProviderFailure::Transport { detail: chain };
    }
    if err.is_timeout() {
        return ProviderFailure::Transport {
            detail: "request timed out".to_string(),
        };
    }
    if err.is_connect() {
        return ProviderFailure::Transport {
            detail: "connection failed".to_string(),
        };
    }
    if err.is_body() || err.is_decode() {
        return ProviderFailure::Transport {
            detail: "response body could not be read".to_string(),
        };
    }
    ProviderFailure::Transport {
        detail: "request failed".to_string(),
    }
}

/// Whether the response carried a `retry-after` header at all, and its delay in
/// whole seconds when it used the delta-seconds form.
///
/// The two answers are separate on purpose. Presence is what decides
/// `retryable`, because *any* `retry-after` — delta-seconds or HTTP-date — is
/// the provider saying to come back later. Reading only the parseable form to
/// decide that flag gets it backwards: an endpoint behind a proxy that emits
/// `Retry-After: Wed, 21 Oct 2026 07:28:00 GMT` would reach the guest as
/// `retryable: false`, which the taxonomy defines as the provider saying the
/// same request will keep failing, and a guest retry loop would abandon a
/// request that was going to succeed.
///
/// The delay itself is still only read from the delta-seconds form, which is
/// what the first-party kinds send. Parsing dates would add a clock dependency
/// for a number nothing currently consumes.
fn retry_after(headers: &reqwest::header::HeaderMap) -> (bool, Option<u64>) {
    let Some(raw) = headers.get("retry-after").and_then(|v| v.to_str().ok()) else {
        return (false, None);
    };
    let secs = raw
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|secs| *secs <= MAX_RETRY_AFTER_SECS);
    (true, secs)
}

#[cfg(test)]
mod tests;
