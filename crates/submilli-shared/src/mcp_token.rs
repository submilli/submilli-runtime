//! Steady-state OAuth for outbound `@mcp/<server>` calls: mint and rotate
//! short-lived **access tokens** from the stored refresh token.
//!
//! The refresh token (and the inputs to redeem it) live in the SecretStore as an
//! [`OAuthCredential`]; access tokens never touch the store — they sit in an
//! in-memory cache keyed on `(blueprint, server)`. Concurrent calls that hit the
//! same expired key coalesce on a per-key lock, so an expiry window costs exactly
//! one token-endpoint round-trip. The MCP transport (SUB-297) drives this:
//! [`OAuthTokenManager::access_token`] before a call, [`OAuthTokenManager::force_refresh`]
//! after a mid-call `401`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use interpreter::runtime::{HttpClient, HttpError, HttpRequest};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::OAuthProvider;
use crate::mcp_auth::{
    CredentialError, OAuthCredential, credential_key, read_credential, write_credential,
};
use crate::secret_store::SecretStore;

/// Refresh this long before the cached token's stated expiry, so the next call
/// doesn't race the boundary — removes a `401`/refresh round-trip from the hot path.
const EXPIRY_SKEW: Duration = Duration::from_secs(60);

/// Access-token lifetime assumed when the token endpoint omits `expires_in`.
const DEFAULT_TTL: Duration = Duration::from_secs(300);

/// Cache lifetime for a static (non-expiring) access token. Long, since it never
/// expires; bounded only so a revoked credential is eventually re-read from the store.
const STATIC_TOKEN_TTL: Duration = Duration::from_secs(3600);

/// Cap on the token-endpoint response we'll buffer.
const MAX_RESPONSE_SIZE: u64 = 64 * 1024;

/// Token-endpoint request timeout (ms).
const EXCHANGE_TIMEOUT_MS: u64 = 30_000;

/// A provider can briefly reject a refresh token it has just issued. Keep the
/// retry delays short enough for discovery's ten-second deadline.
const INVALID_GRANT_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

/// Why minting an access token failed.
#[derive(Debug)]
pub enum McpTokenError {
    /// No credential on file — the blueprint never authenticated this server.
    NotAuthenticated,
    /// The provider rejected the refresh token (`invalid_grant`). Refresh retries
    /// briefly, then retains the credential so a later run can recover. The
    /// transport surfaces exhausted retries as a catchable `McpAuthExpiredError`.
    AuthExpired,
    /// Any other non-success from the token endpoint (network `5xx`, or an OAuth
    /// error that isn't `invalid_grant`), carrying the upstream payload.
    Upstream { status: u16, body: String },
    /// Reading or writing the credential in the SecretStore failed.
    Store(String),
    /// The token-endpoint request didn't complete (transport failure).
    Transport(HttpError),
    /// A `2xx` token-endpoint response we couldn't parse / that carried no access token.
    Malformed(String),
}

impl std::fmt::Display for McpTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpTokenError::NotAuthenticated => {
                write!(
                    f,
                    "no OAuth credential on file; re-run `submilli server mcp authenticate`"
                )
            }
            McpTokenError::AuthExpired => write!(
                f,
                "OAuth token endpoint rejected the refresh token (invalid_grant) after retries; credential retained — retry later or authenticate the MCP server again"
            ),
            McpTokenError::Upstream { status, body } => {
                write!(f, "token endpoint returned HTTP {status}: {body}")
            }
            McpTokenError::Store(msg) => write!(f, "credential store error: {msg}"),
            McpTokenError::Transport(e) => write!(f, "token endpoint request failed: {e}"),
            McpTokenError::Malformed(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for McpTokenError {}

impl From<CredentialError> for McpTokenError {
    fn from(e: CredentialError) -> Self {
        McpTokenError::Store(e.to_string())
    }
}

/// One cached access token and its (skew-adjusted) expiry.
struct Cached {
    access_token: String,
    expires_at: Instant,
}

/// Per-`(blueprint, server)` state behind its own lock — the coalescing point.
#[derive(Default)]
struct Slot {
    cached: Option<Cached>,
}

/// `(blueprint, server)` → its shared slot.
type SlotMap = HashMap<(String, String), Arc<Mutex<Slot>>>;

/// Process-wide access-token cache + refresh, shared across sessions (lives on
/// `AppState`).
pub struct OAuthTokenManager {
    store: Arc<dyn SecretStore>,
    http: Arc<dyn HttpClient>,
    /// Configured OAuth apps — the client_secret for a refresh is resolved from
    /// here (matched by the token endpoint's host), never from the credential.
    providers: Arc<Vec<OAuthProvider>>,
    slots: Mutex<SlotMap>,
}

impl OAuthTokenManager {
    pub fn new(
        store: Arc<dyn SecretStore>,
        http: Arc<dyn HttpClient>,
        providers: Arc<Vec<OAuthProvider>>,
    ) -> Self {
        Self {
            store,
            http,
            providers,
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// A valid bearer token for `(blueprint, server)`: the cached one if still
    /// comfortably unexpired, otherwise freshly refreshed. Concurrent callers for
    /// the same key serialize on its slot lock; the first refresh wins and the
    /// rest reuse it.
    pub async fn access_token(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<String, McpTokenError> {
        let slot = self.slot(blueprint, server).await;
        let mut slot = slot.lock().await;
        if let Some(cached) = &slot.cached
            && Instant::now() + EXPIRY_SKEW < cached.expires_at
        {
            return Ok(cached.access_token.clone());
        }
        self.refresh(blueprint, server, &mut slot).await
    }

    /// Refresh after a mid-call `401`. `stale` is the token that just failed; if
    /// another caller already replaced it, reuse theirs instead of refreshing
    /// again — so a burst of concurrent `401`s collapses to one exchange.
    pub async fn force_refresh(
        &self,
        blueprint: &str,
        server: &str,
        stale: &str,
    ) -> Result<String, McpTokenError> {
        let slot = self.slot(blueprint, server).await;
        let mut slot = slot.lock().await;
        if let Some(cached) = &slot.cached
            && cached.access_token != stale
        {
            return Ok(cached.access_token.clone());
        }
        self.refresh(blueprint, server, &mut slot).await
    }

    /// Replace or remove a credential under the refresh lock, then retire the
    /// cached access token. An in-flight refresh cannot overwrite the new login.
    pub async fn set_credential(
        &self,
        blueprint: &str,
        server: &str,
        credential: Option<&OAuthCredential>,
    ) -> Result<(), McpTokenError> {
        let slot = self.slot(blueprint, server).await;
        let mut slot = slot.lock().await;
        match credential {
            Some(credential) => {
                write_credential(blueprint, server, credential, &self.store).await?;
            }
            None => self
                .store
                .delete(&credential_key(blueprint, server))
                .await
                .map_err(|error| McpTokenError::Store(error.to_string()))?,
        }
        slot.cached = None;
        Ok(())
    }

    /// The slot for a key, created empty on first use.
    async fn slot(&self, blueprint: &str, server: &str) -> Arc<Mutex<Slot>> {
        let mut slots = self.slots.lock().await;
        Arc::clone(
            slots
                .entry((blueprint.to_string(), server.to_string()))
                .or_default(),
        )
    }

    /// Redeem the stored refresh token for a fresh access token and cache it.
    /// Runs under the caller's held slot lock. Persists a rotated refresh token;
    /// keeps the credential on rejection so a later invocation can recover.
    async fn refresh(
        &self,
        blueprint: &str,
        server: &str,
        slot: &mut Slot,
    ) -> Result<String, McpTokenError> {
        let credential = read_credential(blueprint, server, &self.store)
            .await?
            .ok_or(McpTokenError::NotAuthenticated)?;

        // Static-access-token credential (no refresh token): use it as-is, cached
        // long so the slot doesn't re-read the store on every call. A revoked one
        // surfaces as a call-time 401 — the user re-authenticates.
        if let Some(access_token) = &credential.access_token {
            slot.cached = Some(Cached {
                access_token: access_token.clone(),
                expires_at: Instant::now() + STATIC_TOKEN_TTL,
            });
            return Ok(access_token.clone());
        }

        let grant = self.exchange_with_retry(&credential).await?;

        if let Some(rotated) = &grant.refresh_token
            && credential.refresh_token.as_deref() != Some(rotated.as_str())
        {
            let mut next = credential.clone();
            next.refresh_token = Some(rotated.clone());
            write_credential(blueprint, server, &next, &self.store).await?;
        }

        let ttl = grant.expires_in.map_or(DEFAULT_TTL, Duration::from_secs);
        slot.cached = Some(Cached {
            access_token: grant.access_token.clone(),
            expires_at: Instant::now() + ttl,
        });
        Ok(grant.access_token)
    }

    async fn exchange_with_retry(
        &self,
        credential: &OAuthCredential,
    ) -> Result<GrantResponse, McpTokenError> {
        let mut delays = INVALID_GRANT_RETRY_DELAYS.into_iter();
        loop {
            match self.exchange(credential).await {
                Err(McpTokenError::AuthExpired) => {
                    let Some(delay) = delays.next() else {
                        return Err(McpTokenError::AuthExpired);
                    };
                    tokio::time::sleep(delay).await;
                }
                result => return result,
            }
        }
    }

    /// POST `grant_type=refresh_token` to the credential's token endpoint. Pure:
    /// no store writes (the caller owns those). `invalid_grant` maps to
    /// [`McpTokenError::AuthExpired`]; any other non-`2xx` to `Upstream`.
    async fn exchange(&self, credential: &OAuthCredential) -> Result<GrantResponse, McpTokenError> {
        // Only reached for refresh-token credentials (the static path returns
        // earlier in `refresh`), so both fields are present.
        let refresh_token = credential
            .refresh_token
            .as_deref()
            .ok_or_else(|| McpTokenError::Malformed("credential has no refresh token".into()))?;
        let token_endpoint = credential
            .token_endpoint
            .as_deref()
            .ok_or_else(|| McpTokenError::Malformed("credential has no token endpoint".into()))?;
        // The client secret some endpoints need at refresh (e.g. GitHub Apps) comes
        // from the server config, matched by the token endpoint's host — never from
        // the stored credential.
        let client_secret = self.config_client_secret(token_endpoint).await;
        // Build the form body fully before the await — the serializer isn't `Send`.
        let body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "refresh_token");
            form.append_pair("refresh_token", refresh_token);
            if let Some(client_id) = &credential.client_id {
                form.append_pair("client_id", client_id);
            }
            if let Some(secret) = &client_secret {
                form.append_pair("client_secret", secret);
            }
            if !credential.scopes.is_empty() {
                form.append_pair("scope", &credential.scopes.join(" "));
            }
            form.finish().into_bytes()
        };
        let req = HttpRequest {
            method: "POST".to_string(),
            url: token_endpoint.to_string(),
            headers: vec![
                (
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded".to_string(),
                ),
                // GitHub's token endpoint form-encodes its response unless asked
                // for JSON. We still fall back to form parsing below for servers
                // that ignore this.
                ("accept".to_string(), "application/json".to_string()),
            ],
            body,
            timeout_ms: EXCHANGE_TIMEOUT_MS,
            max_response_size: MAX_RESPONSE_SIZE,
            decompress: false,
            transport_policy: None,
        };

        let resp = self
            .http
            .send(&req)
            .await
            .map_err(McpTokenError::Transport)?;
        if (200..300).contains(&resp.status) {
            return parse_grant_response(&resp.body).ok_or_else(|| {
                McpTokenError::Malformed(format!(
                    "token endpoint returned HTTP {} but not a usable token",
                    resp.status
                ))
            });
        }

        let body = String::from_utf8_lossy(&resp.body).into_owned();
        let code = serde_json::from_slice::<OAuthError>(&resp.body)
            .ok()
            .and_then(|e| e.error);
        Err(match code.as_deref() {
            Some("invalid_grant") => McpTokenError::AuthExpired,
            _ => McpTokenError::Upstream {
                status: resp.status,
                body,
            },
        })
    }

    /// The configured client secret for a token endpoint, matched by host and
    /// resolved (literal / `${env.VAR}` / `${secrets.KEY}`). `None` if no provider
    /// matches or it has no secret.
    async fn config_client_secret(&self, token_endpoint: &str) -> Option<String> {
        let provider = crate::mcp::oauth::match_provider(&self.providers, token_endpoint)?;
        let raw = provider.client_secret.as_deref()?;
        crate::mcp::oauth::resolve_secret_ref(raw, Some(&self.store)).await
    }
}

/// A successful token-endpoint response. `access_token` is required, so a `2xx`
/// body without one is a parse error → [`McpTokenError::Malformed`].
#[derive(Debug, Deserialize)]
struct GrantResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Parse a 2xx token response as JSON, falling back to `application/x-www-form-
/// urlencoded` (GitHub form-encodes its token responses). `None` when neither
/// yields a usable `access_token`.
fn parse_grant_response(body: &[u8]) -> Option<GrantResponse> {
    if let Ok(grant) = serde_json::from_slice::<GrantResponse>(body) {
        return Some(grant);
    }
    let mut access_token = None;
    let mut expires_in = None;
    let mut refresh_token = None;
    for (k, v) in url::form_urlencoded::parse(body) {
        match k.as_ref() {
            "access_token" => access_token = Some(v.into_owned()),
            "expires_in" => expires_in = v.parse().ok(),
            "refresh_token" => refresh_token = Some(v.into_owned()),
            _ => {}
        }
    }
    Some(GrantResponse {
        access_token: access_token?,
        expires_in,
        refresh_token,
    })
}

/// The `error` field of an OAuth error response (RFC 6749 §5.2).
#[derive(Debug, Deserialize)]
struct OAuthError {
    #[serde(default)]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use base64::Engine as _;

    #[test]
    fn parse_grant_response_handles_json_and_form() {
        let json = super::parse_grant_response(
            br#"{"access_token":"at","expires_in":3600,"refresh_token":"rt"}"#,
        )
        .expect("json grant");
        assert_eq!(json.access_token, "at");
        assert_eq!(json.expires_in, Some(3600));
        assert_eq!(json.refresh_token.as_deref(), Some("rt"));

        // GitHub form-encodes its token responses.
        let form =
            super::parse_grant_response(b"access_token=at&token_type=bearer&refresh_token=rt")
                .expect("form grant");
        assert_eq!(form.access_token, "at");
        assert_eq!(form.refresh_token.as_deref(), Some("rt"));

        // Neither parse yields an access_token → None.
        assert!(super::parse_grant_response(b"error=bad").is_none());
    }
    use base64::engine::general_purpose::STANDARD;
    use interpreter::stdlib::http::transport::{DownloadMeta, HttpResponse};
    use serde_json::json;

    use super::*;
    use crate::mcp_auth::is_authenticated;
    use crate::secret_store::{FileSecretStore, KeySource};

    const BP: &str = "bp";
    const SRV: &str = "srv";
    const TOKEN_ENDPOINT: &str = "https://idp.example.com/token";

    /// A scripted HTTP stub: each `send` records the request, counts the call,
    /// and returns the next queued `(status, body)` — reusing the last once the
    /// queue is down to one, so steady-state responses repeat.
    struct MockHttp {
        specs: StdMutex<VecDeque<(u16, Vec<u8>)>>,
        calls: AtomicUsize,
        requests: StdMutex<Vec<HttpRequest>>,
        delay: Option<Duration>,
    }

    impl MockHttp {
        fn new(specs: Vec<(u16, Vec<u8>)>, delay: Option<Duration>) -> Arc<Self> {
            Arc::new(Self {
                specs: StdMutex::new(specs.into()),
                calls: AtomicUsize::new(0),
                requests: StdMutex::new(Vec::new()),
                delay,
            })
        }

        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_body(&self) -> String {
            let reqs = self.requests.lock().unwrap();
            String::from_utf8(reqs.last().unwrap().body.clone()).unwrap()
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for MockHttp {
        async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(req.clone());
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            let (status, body) = {
                let mut specs = self.specs.lock().unwrap();
                if specs.len() > 1 {
                    specs.pop_front().unwrap()
                } else {
                    specs
                        .front()
                        .cloned()
                        .expect("at least one scripted response")
                }
            };
            Ok(HttpResponse {
                status,
                status_text: String::new(),
                headers: Vec::new(),
                body,
                final_url: req.url.clone(),
            })
        }

        async fn download(
            &self,
            _req: &HttpRequest,
            _writer: &mut (dyn std::io::Write + Send),
        ) -> Result<DownloadMeta, HttpError> {
            unreachable!("token exchange never downloads")
        }
    }

    fn ok_token(access: &str, expires_in: u64) -> (u16, Vec<u8>) {
        let body =
            json!({ "access_token": access, "token_type": "Bearer", "expires_in": expires_in });
        (200, body.to_string().into_bytes())
    }

    fn ok_token_rotating(access: &str, expires_in: u64, new_refresh: &str) -> (u16, Vec<u8>) {
        let body = json!({
            "access_token": access,
            "expires_in": expires_in,
            "refresh_token": new_refresh,
        });
        (200, body.to_string().into_bytes())
    }

    fn temp_store() -> (tempfile::TempDir, Arc<dyn SecretStore>) {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        std::fs::write(&key_path, STANDARD.encode([3u8; 32])).unwrap();
        let store =
            FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap();
        (tmp, Arc::new(store))
    }

    async fn seed(store: &Arc<dyn SecretStore>, refresh: &str) {
        let credential = OAuthCredential {
            refresh_token: Some(refresh.to_string()),
            access_token: None,
            client_id: Some("cid".to_string()),
            token_endpoint: Some(TOKEN_ENDPOINT.to_string()),
            scopes: vec!["api".to_string()],
        };
        write_credential(BP, SRV, &credential, store).await.unwrap();
    }

    async fn seed_static(store: &Arc<dyn SecretStore>, access: &str) {
        let credential = OAuthCredential {
            refresh_token: None,
            access_token: Some(access.to_string()),
            client_id: None,
            token_endpoint: None,
            scopes: vec![],
        };
        write_credential(BP, SRV, &credential, store).await.unwrap();
    }

    fn manager(store: &Arc<dyn SecretStore>, http: &Arc<MockHttp>) -> OAuthTokenManager {
        OAuthTokenManager::new(
            Arc::clone(store),
            Arc::clone(http) as Arc<dyn HttpClient>,
            Arc::new(Vec::new()),
        )
    }

    #[tokio::test]
    async fn caches_access_token_and_sends_the_right_grant() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(vec![ok_token("at-1", 3600)], None);
        let mgr = manager(&store, &http);

        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert_eq!(http.count(), 1, "second call must hit the cache");

        let body = http.last_body();
        assert!(body.contains("grant_type=refresh_token"), "{body}");
        assert!(body.contains("refresh_token=r1"), "{body}");
        assert!(body.contains("client_id=cid"), "{body}");
        assert!(body.contains("scope=api"), "{body}");
    }

    #[tokio::test(start_paused = true)]
    async fn refreshes_pre_emptively_within_the_skew() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(vec![ok_token("at-1", 120), ok_token("at-2", 120)], None);
        let mgr = manager(&store, &http);

        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        // 90s to expiry (> 60s skew) → still the cached token.
        tokio::time::advance(Duration::from_secs(30)).await;
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert_eq!(http.count(), 1);
        // 40s to expiry (< 60s skew) → refresh ahead of the boundary.
        tokio::time::advance(Duration::from_secs(50)).await;
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-2");
        assert_eq!(http.count(), 2);
    }

    #[tokio::test]
    async fn persists_a_rotated_refresh_token_preserving_the_rest() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(vec![ok_token_rotating("at-1", 3600, "r2")], None);
        let mgr = manager(&store, &http);

        mgr.access_token(BP, SRV).await.unwrap();

        let stored = read_credential(BP, SRV, &store).await.unwrap().unwrap();
        assert_eq!(
            stored.refresh_token.as_deref(),
            Some("r2"),
            "rotated value persisted"
        );
        assert_eq!(stored.client_id.as_deref(), Some("cid"));
        assert_eq!(stored.token_endpoint.as_deref(), Some(TOKEN_ENDPOINT));
        assert_eq!(stored.scopes, vec!["api".to_string()]);
    }

    #[tokio::test]
    async fn static_access_token_is_returned_without_a_token_endpoint_call() {
        let (_tmp, store) = temp_store();
        seed_static(&store, "ghs_static").await;
        // No HTTP responses queued — a token-endpoint call would panic the mock.
        let http = MockHttp::new(vec![], None);
        let mgr = manager(&store, &http);

        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "ghs_static");
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "ghs_static");
        assert_eq!(
            http.count(),
            0,
            "static tokens never hit the token endpoint"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_grant_preserves_credential_for_a_later_run() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let body = json!({ "error": "invalid_grant", "error_description": "revoked" });
        let rejection = (400, body.to_string().into_bytes());
        let http = MockHttp::new(
            vec![
                rejection.clone(),
                rejection.clone(),
                rejection,
                ok_token("at-1", 3600),
            ],
            None,
        );
        let mgr = manager(&store, &http);

        let err = mgr.access_token(BP, SRV).await.unwrap_err();
        assert!(matches!(err, McpTokenError::AuthExpired), "got {err:?}");
        assert_eq!(http.count(), 3, "stop after bounded retries");
        assert_eq!(
            read_credential(BP, SRV, &store)
                .await
                .unwrap()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("r1")
        );
        assert!(is_authenticated(BP, SRV, Some(&store)).await);
        let next_run = manager(&store, &http);
        assert_eq!(next_run.access_token(BP, SRV).await.unwrap(), "at-1");
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_grant_retries_after_a_delay_and_persists_rotation() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(
            vec![
                (400, br#"{"error":"invalid_grant"}"#.to_vec()),
                ok_token_rotating("at-1", 3600, "r2"),
            ],
            None,
        );
        let mgr = manager(&store, &http);
        let start = Instant::now();
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert_eq!(http.count(), 2);
        assert_eq!(
            read_credential(BP, SRV, &store)
                .await
                .unwrap()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("r2")
        );
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert_eq!(http.count(), 2, "successful retry fills the cache");
    }

    #[tokio::test(start_paused = true)]
    async fn rejected_rotated_token_recovers_on_the_last_retry() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let rejection = (400, br#"{"error":"invalid_grant"}"#.to_vec());
        let http = MockHttp::new(
            vec![
                ok_token_rotating("at-1", 3600, "r2"),
                rejection.clone(),
                rejection,
                ok_token_rotating("at-2", 3600, "r3"),
            ],
            None,
        );
        let mgr = Arc::new(manager(&store, &http));
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        let start = Instant::now();
        let mut callers = Vec::new();
        for _ in 0..5 {
            let mgr = Arc::clone(&mgr);
            callers.push(tokio::spawn(async move {
                mgr.force_refresh(BP, SRV, "at-1").await.unwrap()
            }));
        }
        for caller in callers {
            assert_eq!(caller.await.unwrap(), "at-2");
        }
        assert_eq!(start.elapsed(), Duration::from_secs(3));
        assert_eq!(
            http.count(),
            4,
            "concurrent callers share the retry sequence"
        );
        assert!(http.last_body().contains("refresh_token=r2"));
        assert_eq!(
            read_credential(BP, SRV, &store)
                .await
                .unwrap()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("r3")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_retry_preserves_credential() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(
            vec![
                (400, br#"{"error":"invalid_grant"}"#.to_vec()),
                ok_token("at-1", 3600),
            ],
            None,
        );
        let mgr = manager(&store, &http);
        let result =
            tokio::time::timeout(Duration::from_millis(500), mgr.access_token(BP, SRV)).await;
        assert!(result.is_err());
        assert_eq!(http.count(), 1, "cancel during the retry delay");
        assert!(is_authenticated(BP, SRV, Some(&store)).await);
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_stops_on_a_different_upstream_error() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(
            vec![
                (400, br#"{"error":"invalid_grant"}"#.to_vec()),
                (503, b"upstream unavailable".to_vec()),
                ok_token("at-1", 3600),
            ],
            None,
        );
        let mgr = manager(&store, &http);
        assert!(matches!(
            mgr.access_token(BP, SRV).await.unwrap_err(),
            McpTokenError::Upstream { status: 503, .. }
        ));
        assert_eq!(http.count(), 2);
        assert!(is_authenticated(BP, SRV, Some(&store)).await);
    }

    #[tokio::test]
    async fn upstream_error_surfaces_status_and_keeps_credential() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(vec![(503, b"upstream boom".to_vec())], None);
        let mgr = manager(&store, &http);

        match mgr.access_token(BP, SRV).await.unwrap_err() {
            McpTokenError::Upstream { status, body } => {
                assert_eq!(status, 503);
                assert!(body.contains("upstream boom"), "{body}");
            }
            other => panic!("expected Upstream, got {other:?}"),
        }
        // A transient failure must not drop the credential.
        assert!(read_credential(BP, SRV, &store).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn missing_credential_is_not_authenticated_without_an_exchange() {
        let (_tmp, store) = temp_store();
        let http = MockHttp::new(vec![ok_token("at-1", 3600)], None);
        let mgr = manager(&store, &http);

        let err = mgr.access_token(BP, SRV).await.unwrap_err();
        assert!(
            matches!(err, McpTokenError::NotAuthenticated),
            "got {err:?}"
        );
        assert_eq!(http.count(), 0, "no exchange without a credential on file");
    }

    #[tokio::test]
    async fn concurrent_calls_coalesce_to_one_exchange() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        // The delay holds the first refresh in flight while the rest pile up on
        // the per-key lock.
        let http = MockHttp::new(
            vec![ok_token("at-1", 3600)],
            Some(Duration::from_millis(50)),
        );
        let mgr = Arc::new(manager(&store, &http));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let mgr = Arc::clone(&mgr);
            handles.push(tokio::spawn(async move {
                mgr.access_token(BP, SRV).await.unwrap()
            }));
        }
        for handle in handles {
            assert_eq!(handle.await.unwrap(), "at-1");
        }
        assert_eq!(
            http.count(),
            1,
            "an expiry window must coalesce to one exchange"
        );
    }

    #[tokio::test]
    async fn concurrent_force_refresh_after_401_coalesces() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(
            vec![ok_token("at-1", 3600), ok_token("at-2", 3600)],
            Some(Duration::from_millis(50)),
        );
        let mgr = Arc::new(manager(&store, &http));

        // Prime the cache with at-1.
        assert_eq!(mgr.access_token(BP, SRV).await.unwrap(), "at-1");
        assert_eq!(http.count(), 1);

        // Five concurrent 401s, all carrying the now-stale at-1.
        let mut handles = Vec::new();
        for _ in 0..5 {
            let mgr = Arc::clone(&mgr);
            handles.push(tokio::spawn(async move {
                mgr.force_refresh(BP, SRV, "at-1").await.unwrap()
            }));
        }
        for handle in handles {
            assert_eq!(handle.await.unwrap(), "at-2");
        }
        assert_eq!(
            http.count(),
            2,
            "five concurrent 401s trigger exactly one refresh"
        );
    }

    #[tokio::test]
    async fn access_tokens_never_touch_the_store() {
        let (_tmp, store) = temp_store();
        seed(&store, "r1").await;
        let http = MockHttp::new(vec![ok_token("at-secret", 3600)], None);
        let mgr = manager(&store, &http);

        mgr.access_token(BP, SRV).await.unwrap();

        let keys = store.list(Some("mcp_oauth/")).await.unwrap();
        assert_eq!(
            keys,
            vec![credential_key(BP, SRV)],
            "only the credential key may exist under mcp_oauth/"
        );
        let raw = store.get(&credential_key(BP, SRV)).await.unwrap().unwrap();
        assert!(
            !raw.contains("at-secret"),
            "the access token must not be persisted"
        );
    }
}
