//! The outbound HTTP plumbing every remote-provider dispatch (`submilli:llm`,
//! `submilli:embedding`) shares: the credentialed client, bounded body reads,
//! transport-error classification, `retry-after` parsing, and API-key
//! resolution.
//!
//! **Nothing here carries a secret, a URL, or a response body into an error.**
//! Every [`TransportFailure`] holds one of a fixed set of classification strings
//! (plus the network policy's own refusal reason), because reqwest renders the
//! request URL into its `Display` and an operator-supplied endpoint is not
//! something to echo back.

use std::sync::Arc;
use std::time::Duration;

use interpreter::stdlib::http::describe_error_chain;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, interpolate};

use crate::host::BlueprintSecretResolver;
use crate::secret_store::SecretStore;

/// How long one dispatch may take end to end.
///
/// Generous by HTTP standards and deliberately so: a large reasoning request
/// legitimately runs for minutes, and a timeout tuned for an ordinary API would
/// turn a working call into a `transport` failure that costs the operator the
/// tokens anyway. It exists to stop a hung socket pinning a fan-out slot
/// forever, not to bound model latency.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// How long the TCP/TLS handshake may take, separately from the request as a
/// whole.
///
/// Establishing a connection is not where a model spends its time, so this can
/// be short where [`REQUEST_TIMEOUT`] cannot. Without it, an endpoint whose
/// address is routable-but-dead — a typo'd `base_url`, a decommissioned host —
/// holds a fan-out slot for the OS's own SYN-retry budget, which is over a
/// minute on most platforms, before the generous overall timeout even begins to
/// apply.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A `retry-after` beyond this is treated as absent. The header is advisory and
/// the value only feeds the `retryable` flag, so an absurd one should not make a
/// guest wait on a number no provider meant.
pub const MAX_RETRY_AFTER_SECS: u64 = 3600;

/// The most of one response this process will buffer.
///
/// A completion is text bounded by the model's own output cap, so this is far
/// above any legitimate answer; it exists because the peer on an
/// operator-supplied endpoint is not necessarily one to trust to bound its own
/// body.
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// The classification string for a request that timed out, so a caller can tell
/// a timeout from other transport failures.
pub const TIMEOUT_DETAIL: &str = "request timed out";

/// A transport-level failure, as a fixed classification string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFailure {
    pub detail: String,
    /// Refused on this side, before anything reached the wire: the network policy, or a
    /// request that could not be built.
    pub local: bool,
}

impl TransportFailure {
    fn fixed(detail: &str) -> Self {
        Self {
            detail: detail.to_string(),
            local: false,
        }
    }
}

/// A declared `${secrets.X}` key that would not resolve, or resolved empty.
///
/// Carries nothing: neither the secret's name nor the resolver's message may
/// escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyUnresolved;

/// Apply the settings every remote-provider client shares to `builder`.
///
/// No cookie store is ever enabled, so the client carries no cross-request state
/// and one pool is safe to share across the elements of a fan-out.
pub fn configure_client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    builder
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        // Redirects are refused outright, which is stricter than the
        // `submilli:http` client and deliberately so.
        //
        // reqwest's default follows up to ten hops and strips only
        // `Authorization` when the host changes. Several providers authenticate
        // with a *custom* header — Anthropic's `x-api-key`, Google's
        // `x-goog-api-key` — which survives the hop, so a
        // `307 Location: https://attacker/` from any endpoint would hand over the
        // operator's key and the request body. The exposure is inverted from
        // intuition: the custom-header kinds are the vulnerable ones, and the
        // `authorization`-header kinds are only incidentally safe.
        //
        // No provider endpoint legitimately redirects, so a redirect is refused
        // rather than followed to a same-host-only policy: a `3xx` here means the
        // endpoint is not what the operator declared, and that is worth surfacing
        // as a transport failure rather than quietly chasing.
        .redirect(reqwest::redirect::Policy::none())
}

/// Resolve a row's `${secrets.X}` into the actual key.
///
/// A row that declares no key resolves to `None` rather than an error: a
/// locally-hosted endpoint may genuinely need none, and inventing a credential
/// requirement it does not have would refuse a working deployment. A
/// declared-but-unresolvable key *is* an error.
pub async fn resolve_api_key(
    reference: Option<&str>,
    blueprint: &Blueprint,
    secret_store: Option<Arc<dyn SecretStore>>,
    harness_secrets: Arc<HarnessSecretBindings>,
) -> Result<Option<String>, KeyUnresolved> {
    let Some(reference) = reference else {
        return Ok(None);
    };
    let resolver = BlueprintSecretResolver::with_harness(secret_store, harness_secrets);
    match interpolate(reference, blueprint, &resolver).await {
        Ok(key) if key.is_empty() => Err(KeyUnresolved),
        Ok(key) => Ok(Some(key)),
        Err(_) => Err(KeyUnresolved),
    }
}

/// Read the body, refusing to buffer more than [`MAX_RESPONSE_BYTES`].
///
/// Operator-supplied endpoints mean the peer is not necessarily one this process
/// should trust to bound its own response. The interpreter's outbound HTTP
/// client caps bodies for the same reason; this is the same guarantee on the
/// same kind of connection, and it stops reading rather than reading and then
/// measuring.
///
/// The overflow is a transport failure carrying only a fixed string — no part of
/// the body it refused to finish reading.
pub async fn read_bounded(response: reqwest::Response) -> Result<String, TransportFailure> {
    use futures::StreamExt as _;

    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_transport_error)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(TransportFailure::fixed("response exceeded the size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| TransportFailure::fixed("response body was not valid UTF-8"))
}

/// The phrase the network policy's refusal carries, wherever it surfaces.
///
/// Embedding dispatch matches this detail text, so [`map_transport_error`] must
/// keep emitting it.
pub const BLOCKED_BY_POLICY_DETAIL: &str = "blocked by network policy";

/// Map a transport-level reqwest failure.
///
/// `detail` is one of a fixed set of classification strings, never the error's
/// own message: reqwest renders the URL into its `Display`, and an
/// operator-supplied URL is not something to echo.
pub fn map_transport_error(err: reqwest::Error) -> TransportFailure {
    // A resolver refusal surfaces as a connect error; the policy's reason sits at
    // the bottom of the chain and is what the operator needs to see.
    let chain = describe_error_chain(&err);
    if chain.contains(BLOCKED_BY_POLICY_DETAIL) {
        return TransportFailure {
            detail: chain,
            local: true,
        };
    }
    // The request could not be built (an unusable header from a credential, say).
    if err.is_builder() {
        return TransportFailure {
            local: true,
            ..TransportFailure::fixed("request failed")
        };
    }
    if err.is_timeout() {
        return TransportFailure::fixed(TIMEOUT_DETAIL);
    }
    if err.is_connect() {
        return TransportFailure::fixed("connection failed");
    }
    if err.is_body() || err.is_decode() {
        return TransportFailure::fixed("response body could not be read");
    }
    TransportFailure::fixed("request failed")
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
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> (bool, Option<u64>) {
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
