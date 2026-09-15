//! Embedder glue that adapts a [`Blueprint`] to the runtime's host-service
//! traits ([`SecurityCheck`], [`AuthProxy`], [`SecretProvider`]). The policy
//! logic, filter matching, and `${secrets.X}` interpolation live in
//! `submilli-blueprint`; this is the thin `interpreter`-facing layer that an
//! embedder (the server, or local-dev `submilli run --blueprint`) installs onto
//! a `StoreData`. It lives here rather than in `submilli-blueprint` so that
//! crate stays free of the `interpreter` dependency.

use std::sync::Arc;

use async_trait::async_trait;
use interpreter::runtime::{
    AuthProxy, AuthProxyError, CheckOutcome, HttpRequest, SecretProvider, SecurityCheck,
};
use url::Url;

use std::collections::BTreeMap;

use submilli_blueprint::{
    Action, AuthError, Blueprint, HarnessSecretBindings, Injections, SecretResolver, SecretSource,
    VarBindings, resolve_injections,
};

/// The package name auth-proxy injection is scoped to. Matches the name the runtime gives a
/// user script's module, which is what it reads off the running wasm frame to identify a
/// caller.
const MAIN_PACKAGE: &str = "main";

// ---- SecurityCheck -------------------------------------------------------

/// Blueprint-driven [`SecurityCheck`]: evaluates the blueprint's `permissions:`
/// rules for each capability check.
///
/// Deny-by-default: a capability is allowed only when a rule says so, and a
/// policy-free blueprint (no `permissions:`, no `default:`) denies every
/// capability. `default: allow` inverts this to allow-by-default. `ask-human`
/// is deferred (no suspend/resume yet), so it resolves to `Deny` with a reason
/// that says so.
pub struct PolicyCheck {
    blueprint: Arc<Blueprint>,
    /// Caller-supplied `${vars.NAME}` bindings for this session, resolved at
    /// session init. Empty for callers that bind no variables.
    variables: Arc<VarBindings>,
}

impl PolicyCheck {
    pub fn new(blueprint: Arc<Blueprint>) -> Self {
        Self::with_variables(blueprint, Arc::new(BTreeMap::new()))
    }

    /// As [`Self::new`], with the session's resolved variable bindings threaded
    /// into every `${vars.NAME}` filter operand.
    pub fn with_variables(blueprint: Arc<Blueprint>, variables: Arc<VarBindings>) -> Self {
        PolicyCheck {
            blueprint,
            variables,
        }
    }
}

impl SecurityCheck for PolicyCheck {
    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        match self
            .blueprint
            .resolve_permission(caller, capability, context, &self.variables)
        {
            Action::Allow => CheckOutcome::Allow,
            Action::Deny => CheckOutcome::Deny {
                reason: format!("policy denied {capability} for {caller}"),
            },
            Action::AskHuman => CheckOutcome::Deny {
                reason: format!(
                    "policy requires human approval for {capability} (caller {caller}); \
                     ask-human is deferred and treated as deny"
                ),
            },
        }
    }
}

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
/// embedder's; this trait is the contract [`EnvFileSecretResolver`] resolves
/// `store:` lookups through.
#[async_trait]
pub trait SecretStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError>;
    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError>;
    async fn delete(&self, key: &str) -> Result<(), SecretStoreError>;
    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError>;
}

/// Resolves declared secrets from the environment, mounted files, the
/// configured [`SecretStore`], or a session's trusted harness bindings.
/// [`Self::new`] installs an empty harness binding set for local/operator flows.
pub struct EnvFileSecretResolver {
    store: Option<Arc<dyn SecretStore>>,
    harness: Arc<HarnessSecretBindings>,
}

impl EnvFileSecretResolver {
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
impl SecretResolver for EnvFileSecretResolver {
    async fn resolve(&self, name: &str, source: &SecretSource) -> Result<String, AuthError> {
        match source {
            SecretSource::Env(var) => {
                std::env::var(var).map_err(|_| AuthError::MissingSecret(name.to_string()))
            }
            SecretSource::File(path) => std::fs::read_to_string(path)
                .map(|s| s.trim().to_string())
                .map_err(|_| AuthError::MissingSecret(name.to_string())),
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
    resolver: EnvFileSecretResolver,
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
            resolver: EnvFileSecretResolver::with_harness(store, harness),
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
    blueprint: Arc<Blueprint>,
    resolver: EnvFileSecretResolver,
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
        Self {
            blueprint,
            resolver: EnvFileSecretResolver::with_harness(store, harness),
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
        // main-only: library code never gets the operator's auth injected.
        if caller != MAIN_PACKAGE {
            return Ok(req);
        }
        // An unparseable URL has nothing to match; let the http layer report it.
        let Some(host) = Url::parse(&req.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
        else {
            return Ok(req);
        };
        let Some(injections) = resolve_injections(&self.blueprint, &host, &self.resolver)
            .await
            .map_err(to_proxy_error)?
        else {
            return Ok(req);
        };
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
        }
    }

    fn header_proxy(env_var: &str) -> BlueprintAuthProxy {
        let yaml = format!(
            "name: x\nsecrets:\n  K: {{ env: {env_var} }}\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${{secrets.K}}\"\n"
        );
        BlueprintAuthProxy::new(Arc::new(parse(&yaml).unwrap()), None)
    }

    fn header_value<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[tokio::test]
    async fn injects_resolved_header_for_main() {
        // SAFETY: unique var name per test; reads happen on this thread only.
        unsafe { std::env::set_var("SUB_AP_TEST_INJECT", "sekret") };
        let proxy = header_proxy("SUB_AP_TEST_INJECT");
        let out = proxy
            .transform(req("https://api.example.com/x", vec![]), "main")
            .await
            .unwrap();
        assert_eq!(header_value(&out, "authorization"), Some("Bearer sekret"));
    }

    #[tokio::test]
    async fn skips_non_main_caller() {
        unsafe { std::env::set_var("SUB_AP_TEST_LIB", "sekret") };
        let proxy = header_proxy("SUB_AP_TEST_LIB");
        let out = proxy
            .transform(req("https://api.example.com/x", vec![]), "submilli:http")
            .await
            .unwrap();
        assert!(header_value(&out, "authorization").is_none());
    }

    #[tokio::test]
    async fn no_rule_for_host_passes_through() {
        unsafe { std::env::set_var("SUB_AP_TEST_NOHOST", "sekret") };
        let proxy = header_proxy("SUB_AP_TEST_NOHOST");
        let out = proxy
            .transform(req("https://other.host/x", vec![]), "main")
            .await
            .unwrap();
        assert!(header_value(&out, "authorization").is_none());
    }

    #[tokio::test]
    async fn injected_header_overrides_script_header() {
        unsafe { std::env::set_var("SUB_AP_TEST_OVERRIDE", "sekret") };
        let proxy = header_proxy("SUB_AP_TEST_OVERRIDE");
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
    async fn missing_env_secret_errors() {
        unsafe { std::env::remove_var("SUB_AP_TEST_ABSENT") };
        let proxy = header_proxy("SUB_AP_TEST_ABSENT");
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
        unsafe { std::env::set_var("SUB_AP_TEST_QUERY", "qval") };
        let yaml = "name: x\nsecrets:\n  K: { env: SUB_AP_TEST_QUERY }\nauth_proxy:\n  - host: api.example.com\n    query:\n      appid: \"${secrets.K}\"\n";
        let proxy = BlueprintAuthProxy::new(Arc::new(parse(yaml).unwrap()), None);
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
