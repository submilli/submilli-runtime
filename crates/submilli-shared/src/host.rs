//! Embedder glue that adapts a [`Blueprint`] to the runtime's host-service
//! traits (`SecurityCheck`, [`AuthProxy`], [`SecretProvider`]). The policy
//! check lives in `submilli-policy`, and `${secrets.X}` interpolation in
//! `submilli-blueprint`; this is the thin `interpreter`-facing layer that an
//! embedder (the server, or local-dev `submilli run --blueprint`) installs onto
//! a `StoreData`. It lives here rather than in `submilli-blueprint` so that
//! crate stays free of the `interpreter` dependency.

use std::sync::Arc;

use async_trait::async_trait;
use interpreter::runtime::{AuthProxy, AuthProxyError, HttpRequest, SecretProvider};
use interpreter::stdlib::http::HttpTransportPolicy;
use url::Url;

use submilli_blueprint::{
    AuthError, Blueprint, HarnessSecretBindings, Injections, SecretResolver, SecretSource,
    resolve_injections,
};

/// The [`submilli_policy::PermissionCheck`] over a blueprint's `permissions:`
/// rules and `default:`, evaluated in place.
pub type PolicyCheck = submilli_policy::PermissionCheck<Blueprint>;

/// The package name auth-proxy injection is scoped to. Matches the name the runtime gives a
/// user script's module, which is what it reads off the running wasm frame to identify a
/// caller.
const MAIN_PACKAGE: &str = "main";

// ---- SecretStore ---------------------------------------------------------

#[derive(Debug)]
pub enum SecretStoreError {
    Io(String),
    /// Seal/open failure, or a key that isn't valid base64 / 32 bytes.
    Crypto(String),
    /// The key source is missing or unreadable at store construction.
    KeyConfig(String),
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretStoreError::Io(msg) => write!(f, "secret store io: {msg}"),
            SecretStoreError::Crypto(msg) => write!(f, "secret store crypto: {msg}"),
            SecretStoreError::KeyConfig(msg) => write!(f, "secret store key: {msg}"),
        }
    }
}

impl std::error::Error for SecretStoreError {}

/// Backend for credential-shaped state behind the blueprint `store:` secret
/// source. The concrete implementation (encrypted file store, KMS, …) is the
/// embedder's; this trait is the contract [`BlueprintSecretResolver`] resolves
/// `store:` lookups through.
#[async_trait]
pub trait SecretStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError>;
    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError>;
    /// Remove a key, returning whether it existed.
    async fn delete(&self, key: &str) -> Result<bool, SecretStoreError>;
    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError>;
}

/// Resolves declared secrets from the configured [`SecretStore`] or a session's
/// trusted harness bindings.
/// [`Self::new`] installs an empty harness binding set for local/operator flows.
pub struct BlueprintSecretResolver {
    store: Option<Arc<dyn SecretStore>>,
    harness: Arc<HarnessSecretBindings>,
}

impl BlueprintSecretResolver {
    pub fn new(store: Option<Arc<dyn SecretStore>>) -> Self {
        Self::with_harness(store, Arc::new(HarnessSecretBindings::new()))
    }

    pub fn with_harness(
        store: Option<Arc<dyn SecretStore>>,
        harness: Arc<HarnessSecretBindings>,
    ) -> Self {
        Self { store, harness }
    }
}

#[async_trait]
impl SecretResolver for BlueprintSecretResolver {
    async fn resolve(&self, name: &str, source: &SecretSource) -> Result<String, AuthError> {
        match source {
            // On-demand decrypt: reads + opens the one sealed file, awaited so the
            // executor isn't blocked.
            SecretSource::Store(key) => match &self.store {
                None => Err(AuthError::Other(format!(
                    "secret '{name}': no secret store is configured on this server"
                ))),
                Some(store) => match store.get(key).await {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) => Err(AuthError::MissingSecret(name.to_string())),
                    Err(err) => Err(AuthError::Other(format!(
                        "secret '{name}': store error: {err}"
                    ))),
                },
            },
            SecretSource::Harness(_) => self
                .harness
                .get(name)
                .cloned()
                .ok_or_else(|| AuthError::MissingSecret(name.to_string())),
        }
    }
}

/// Blueprint-backed provider for script-visible `submilli:secrets.get`.
pub struct BlueprintSecretProvider {
    blueprint: Arc<Blueprint>,
    resolver: BlueprintSecretResolver,
}

impl BlueprintSecretProvider {
    pub fn new(blueprint: Arc<Blueprint>, store: Option<Arc<dyn SecretStore>>) -> Self {
        Self::with_harness(blueprint, store, Arc::new(HarnessSecretBindings::new()))
    }

    pub fn with_harness(
        blueprint: Arc<Blueprint>,
        store: Option<Arc<dyn SecretStore>>,
        harness: Arc<HarnessSecretBindings>,
    ) -> Self {
        Self {
            blueprint,
            resolver: BlueprintSecretResolver::with_harness(store, harness),
        }
    }
}

impl SecretProvider for BlueprintSecretProvider {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
    > {
        Box::pin(async move {
            let Some(source) = self.blueprint.secrets.get(name) else {
                return Ok(None);
            };
            match self.resolver.resolve(name, source).await {
                Ok(value) => Ok(Some(value)),
                Err(AuthError::UndeclaredSecret(_) | AuthError::MissingSecret(_)) => Ok(None),
                Err(AuthError::Other(msg)) => Err(msg),
            }
        })
    }
}

// ---- AuthProxy -----------------------------------------------------------

/// [`AuthProxy`] backed by a blueprint's `auth_proxy:` + `secrets:` blocks.
/// Injects the matching rule's headers / query params (with `${secrets.X}`
/// resolved) into outbound `submilli:http` calls made by `main`.
pub struct BlueprintAuthProxy {
    transport_policy: Arc<HttpTransportPolicy>,
    authenticated_policy: Arc<HttpTransportPolicy>,
    blueprint: Arc<Blueprint>,
    resolver: BlueprintSecretResolver,
}

impl BlueprintAuthProxy {
    pub fn new(blueprint: Arc<Blueprint>, store: Option<Arc<dyn SecretStore>>) -> Self {
        Self::with_harness(blueprint, store, Arc::new(HarnessSecretBindings::new()))
    }

    pub fn with_harness(
        blueprint: Arc<Blueprint>,
        store: Option<Arc<dyn SecretStore>>,
        harness: Arc<HarnessSecretBindings>,
    ) -> Self {
        let transport_policy = HttpTransportPolicy {
            allow_insecure_http: blueprint.allow_insecure_http,
            auth_proxy_hosts: blueprint
                .auth_proxy
                .iter()
                .map(|rule| (rule.host.clone(), rule.allow_insecure_http))
                .collect(),
            same_origin_redirects: false,
        };
        let authenticated_policy = Arc::new(HttpTransportPolicy {
            same_origin_redirects: true,
            ..transport_policy.clone()
        });
        Self {
            transport_policy: Arc::new(transport_policy),
            authenticated_policy,
            blueprint,
            resolver: BlueprintSecretResolver::with_harness(store, harness),
        }
    }
}

#[async_trait]
impl AuthProxy for BlueprintAuthProxy {
    async fn transform(
        &self,
        mut req: HttpRequest,
        caller: &str,
    ) -> Result<HttpRequest, AuthProxyError> {
        req.transport_policy = Some(Arc::clone(&self.transport_policy));
        // Check every caller before resolving credentials, including package calls.
        let url =
            Url::parse(&req.url).map_err(|_| AuthProxyError::Other("invalid HTTP URL".into()))?;
        self.transport_policy
            .check_destination(&url)
            .map_err(|error| AuthProxyError::Other(error.to_string()))?;
        if caller != MAIN_PACKAGE {
            return Ok(req);
        }
        let Some(host) = url.host_str() else {
            return Ok(req);
        };
        let Some(injections) = resolve_injections(&self.blueprint, host, &self.resolver)
            .await
            .map_err(to_proxy_error)?
        else {
            return Ok(req);
        };
        req.transport_policy = Some(Arc::clone(&self.authenticated_policy));
        apply(&mut req, injections);
        Ok(req)
    }
}

/// Apply injections; the blueprint value overrides any the script set itself.
fn apply(req: &mut HttpRequest, injections: Injections) {
    for (name, value) in injections.headers {
        let key = name.to_ascii_lowercase(); // host stores header names lowercased
        req.headers.retain(|(k, _)| *k != key);
        req.headers.push((key, value));
    }
    if !injections.query.is_empty()
        && let Ok(url) = Url::parse(&req.url)
    {
        req.url = with_query(&url, &injections.query);
    }
}

/// Rebuild the URL's query string: keep the script's params except those the
/// blueprint sets (blueprint wins), then append the injected ones.
fn with_query(url: &Url, injected: &[(String, String)]) -> String {
    let overridden: Vec<&str> = injected.iter().map(|(k, _)| k.as_str()).collect();
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !overridden.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    {
        let mut pairs = out.query_pairs_mut();
        pairs.clear();
        for (k, v) in &kept {
            pairs.append_pair(k, v);
        }
        for (k, v) in injected {
            pairs.append_pair(k, v);
        }
    }
    // `query_pairs_mut().clear()` on an originally-empty query leaves a bare `?`.
    if out.query() == Some("") {
        out.set_query(None);
    }
    out.to_string()
}

fn to_proxy_error(err: AuthError) -> AuthProxyError {
    match err {
        AuthError::UndeclaredSecret(name) => AuthProxyError::UndeclaredSecret(name),
        AuthError::MissingSecret(name) => AuthProxyError::MissingSecret(name),
        AuthError::Other(msg) => AuthProxyError::Other(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use submilli_blueprint::parse;

    fn req(url: &str, headers: Vec<(&str, &str)>) -> HttpRequest {
        HttpRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: Vec::new(),
            timeout_ms: 1000,
            max_response_size: 1024,
            decompress: false,
            transport_policy: None,
            redirect_guard: None,
            recorded_as: None,
        }
    }

    fn header_proxy(value: Option<&str>) -> BlueprintAuthProxy {
        let yaml = "name: x\nsecrets:\n  K: { harness: {} }\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.K}\"\n";
        let bindings = value
            .map(|value| ("K".into(), value.into()))
            .into_iter()
            .collect();
        BlueprintAuthProxy::with_harness(Arc::new(parse(yaml).unwrap()), None, Arc::new(bindings))
    }

    fn header_value<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[tokio::test]
    async fn insecure_http_requires_both_flags_before_resolving_secrets() {
        // An unbound harness secret proves denial occurs before secret resolution.
        for blueprint in [false, true] {
            for rule in [false, true] {
                let yaml = format!(
                    "name: gates\nallow_insecure_http: {blueprint}\nsecrets:\n  K: {{ harness: {{}} }}\nauth_proxy:\n- host: example.com\n  allow_insecure_http: {rule}\n  auth: {{ bearer: K }}\n"
                );
                let proxy = BlueprintAuthProxy::new(Arc::new(parse(&yaml).unwrap()), None);
                for caller in ["main", "@test/package"] {
                    let result = proxy
                        .transform(req("http://example.com/", vec![]), caller)
                        .await;
                    if blueprint && rule {
                        if caller == "main" {
                            assert!(matches!(result, Err(AuthProxyError::MissingSecret(_))));
                        } else {
                            let request = result.unwrap();
                            assert!(request.headers.is_empty());
                            assert!(!request.transport_policy.unwrap().same_origin_redirects);
                        }
                    } else {
                        let error = result.unwrap_err().to_string();
                        assert!(error.contains("HTTPS required"), "{error}");
                        assert!(error.contains(if blueprint {
                            "auth_proxy rule"
                        } else {
                            "blueprint"
                        }));
                    }
                }
                let unmatched = proxy
                    .transform(req("http://other.com/", vec![]), "main")
                    .await;
                assert_eq!(unmatched.is_ok(), blueprint);
                assert!(
                    proxy
                        .transform(req("https://other.com/", vec![]), "main")
                        .await
                        .is_ok()
                );
            }
        }
    }

    #[tokio::test]
    async fn allowed_injections_lock_redirects_and_preserve_first_rule() {
        for injection in [
            "auth: { bearer: K }",
            "auth: { basic: { username: alice, password: K } }",
            "headers: { X-Key: '${secrets.K}' }",
            "query: { key: '${secrets.K}' }",
        ] {
            let yaml = format!(
                "name: gates\nallow_insecure_http: true\nsecrets:\n  K: {{ harness: {{ required: true }} }}\nauth_proxy:\n- host: example.com\n  allow_insecure_http: true\n  {injection}\n- host: example.com\n  headers: {{ X-Second: ignored }}\n"
            );
            let proxy = BlueprintAuthProxy::with_harness(
                Arc::new(parse(&yaml).unwrap()),
                None,
                Arc::new(BTreeMap::from([("K".into(), "test-token".into())])),
            );
            for scheme in ["http", "https"] {
                let result = proxy
                    .transform(req(&format!("{scheme}://example.com/"), vec![]), "main")
                    .await
                    .unwrap();
                assert!(
                    result
                        .transport_policy
                        .as_ref()
                        .unwrap()
                        .same_origin_redirects
                );
                assert!(!result.headers.is_empty() || result.url.contains("test-token"));
                assert!(header_value(&result, "x-second").is_none());
            }
        }
    }

    #[tokio::test]
    async fn injects_resolved_header_for_main() {
        let proxy = header_proxy(Some("sekret"));
        let out = proxy
            .transform(req("https://api.example.com/x", vec![]), "main")
            .await
            .unwrap();
        assert_eq!(header_value(&out, "authorization"), Some("Bearer sekret"));
    }

    #[tokio::test]
    async fn skips_non_main_caller() {
        let proxy = header_proxy(Some("sekret"));
        let out = proxy
            .transform(req("https://api.example.com/x", vec![]), "submilli:http")
            .await
            .unwrap();
        assert!(header_value(&out, "authorization").is_none());
    }

    #[tokio::test]
    async fn no_rule_for_host_passes_through() {
        let proxy = header_proxy(Some("sekret"));
        let out = proxy
            .transform(req("https://other.host/x", vec![]), "main")
            .await
            .unwrap();
        assert!(header_value(&out, "authorization").is_none());
    }

    #[tokio::test]
    async fn injected_header_overrides_script_header() {
        let proxy = header_proxy(Some("sekret"));
        let out = proxy
            .transform(
                req(
                    "https://api.example.com/x",
                    vec![("authorization", "user-set")],
                ),
                "main",
            )
            .await
            .unwrap();
        let auths: Vec<&str> = out
            .headers
            .iter()
            .filter(|(k, _)| k == "authorization")
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(auths, vec!["Bearer sekret"], "blueprint wins, no duplicate");
    }

    #[tokio::test]
    async fn missing_harness_secret_errors() {
        let proxy = header_proxy(None);
        let err = proxy
            .transform(req("https://api.example.com/x", vec![]), "main")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthProxyError::MissingSecret(n) if n == "K"));
    }

    #[tokio::test]
    async fn harness_bindings_are_resolved_per_provider() {
        let blueprint = Arc::new(
            parse("name: x\nsecrets:\n  K:\n    harness:\n      required: false\n").unwrap(),
        );
        let first = BlueprintSecretProvider::with_harness(
            Arc::clone(&blueprint),
            None,
            Arc::new(BTreeMap::from([("K".into(), "first".into())])),
        );
        let second = BlueprintSecretProvider::with_harness(
            blueprint,
            None,
            Arc::new(BTreeMap::from([("K".into(), "second".into())])),
        );
        assert_eq!(first.get("K").await.unwrap().as_deref(), Some("first"));
        assert_eq!(second.get("K").await.unwrap().as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn injects_query_param_overriding_script() {
        let yaml = "name: x\nsecrets:\n  K: { harness: {} }\nauth_proxy:\n  - host: api.example.com\n    query:\n      appid: \"${secrets.K}\"\n";
        let proxy = BlueprintAuthProxy::with_harness(
            Arc::new(parse(yaml).unwrap()),
            None,
            Arc::new(BTreeMap::from([("K".into(), "qval".into())])),
        );
        let out = proxy
            .transform(
                req("https://api.example.com/x?appid=script&keep=1", vec![]),
                "main",
            )
            .await
            .unwrap();
        let url = Url::parse(&out.url).unwrap();
        let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q.get("appid").map(String::as_str), Some("qval"));
        assert_eq!(q.get("keep").map(String::as_str), Some("1"));
    }
}
