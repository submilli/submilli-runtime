//! Embedder glue that wires a [`Blueprint`] into the runtime's host-service
//! traits ([`AuthProxy`], [`SecretProvider`], and `SecurityCheck` through
//! [`PolicyCheck`]). The adapters themselves live in `submilli-policy`; this
//! ties them to the blueprint's `secrets:` block and the server's secret store.

use std::sync::Arc;

use async_trait::async_trait;
use interpreter::runtime::{AuthProxy, AuthProxyError, HttpRequest, SecretProvider};
use submilli_blueprint::{
    AuthError, Blueprint, DeclaredSecrets, HarnessSecretBindings, SecretSource,
    SourceSecretResolver,
};
use submilli_policy::SecretResolver;
use submilli_policy::host::{PolicyAuthProxy, PolicySecretProvider};

pub use submilli_policy::host::PolicyCheck;

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
impl SourceSecretResolver for BlueprintSecretResolver {
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

/// A blueprint's declared secrets as a policy [`SecretResolver`]: a name the
/// `secrets:` block does not declare is undeclared, and a declared one resolves
/// from its source.
pub struct BlueprintSecrets {
    blueprint: Arc<Blueprint>,
    resolver: BlueprintSecretResolver,
}

impl BlueprintSecrets {
    pub fn new(blueprint: Arc<Blueprint>, resolver: BlueprintSecretResolver) -> Self {
        Self {
            blueprint,
            resolver,
        }
    }
}

#[async_trait]
impl SecretResolver for BlueprintSecrets {
    async fn resolve(&self, name: &str) -> Result<String, AuthError> {
        DeclaredSecrets {
            blueprint: &self.blueprint,
            inner: &self.resolver,
        }
        .resolve(name)
        .await
    }
}

/// Blueprint-backed provider for script-visible `submilli:secrets.get`.
pub struct BlueprintSecretProvider(PolicySecretProvider);

impl BlueprintSecretProvider {
    pub fn new(blueprint: Arc<Blueprint>, store: Option<Arc<dyn SecretStore>>) -> Self {
        Self::with_harness(blueprint, store, Arc::new(HarnessSecretBindings::new()))
    }

    pub fn with_harness(
        blueprint: Arc<Blueprint>,
        store: Option<Arc<dyn SecretStore>>,
        harness: Arc<HarnessSecretBindings>,
    ) -> Self {
        let secrets = BlueprintSecrets::new(
            blueprint,
            BlueprintSecretResolver::with_harness(store, harness),
        );
        Self(PolicySecretProvider::new(Arc::new(secrets)))
    }
}

impl SecretProvider for BlueprintSecretProvider {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
    > {
        self.0.get(name)
    }
}

// ---- AuthProxy -----------------------------------------------------------

/// [`AuthProxy`] backed by a blueprint's `auth_proxy:` + `secrets:` blocks.
/// Injects the matching rule's headers / query params (with `${secrets.X}`
/// resolved) into outbound `submilli:http` calls made by `main`.
pub struct BlueprintAuthProxy(PolicyAuthProxy);

impl BlueprintAuthProxy {
    pub fn new(blueprint: Arc<Blueprint>, store: Option<Arc<dyn SecretStore>>) -> Self {
        Self::with_harness(blueprint, store, Arc::new(HarnessSecretBindings::new()))
    }

    pub fn with_harness(
        blueprint: Arc<Blueprint>,
        store: Option<Arc<dyn SecretStore>>,
        harness: Arc<HarnessSecretBindings>,
    ) -> Self {
        let policy = blueprint.auth_proxy_policy();
        let secrets = BlueprintSecrets::new(
            blueprint,
            BlueprintSecretResolver::with_harness(store, harness),
        );
        Self(PolicyAuthProxy::new(policy, Arc::new(secrets)))
    }
}

#[async_trait]
impl AuthProxy for BlueprintAuthProxy {
    async fn transform(
        &self,
        req: HttpRequest,
        caller: &str,
    ) -> Result<HttpRequest, AuthProxyError> {
        self.0.transform(req, caller).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use interpreter::runtime::{
        CheckOutcome, DecisionAction, DecisionCause, DecisionExplanation, FailureReasonRecord,
        RuleCitation, SecurityCheck,
    };
    use submilli_blueprint::{Policy, parse};
    use url::Url;

    use super::*;

    fn policy_of(blueprint: Blueprint) -> Arc<Policy> {
        Arc::new(blueprint.policy())
    }

    /// Call-site derivation normalizes the same fields from the catalog, so a
    /// derived `requires` filter names the path this check sees.
    #[test]
    fn working_directory_is_used_for_all_filesystem_policy_fields() {
        let blueprint = parse("name: cwd\ndefault: deny\npermissions:\n  main:\n    - {capability: fs.read, action: allow, filter: 'path glob \"/notes/*\"'}\n    - {capability: fs.move, action: allow, filter: 'from glob \"/notes/*\" and to glob \"/notes/*\"'}\n    - {capability: http.download, action: allow, filter: 'vfs_path glob \"/notes/*\"'}\n").unwrap();
        let policy = PolicyCheck::new(policy_of(blueprint));
        for (capability, context) in [
            ("fs.read", serde_json::json!({"path":"a"})),
            ("fs.move", serde_json::json!({"from":"a", "to":"b"})),
            ("http.download", serde_json::json!({"vfs_path":"a"})),
        ] {
            assert!(matches!(
                policy.check_with_cwd("main", capability, &context, "/notes"),
                CheckOutcome::Allow { .. }
            ));
            assert!(matches!(
                policy.check_with_cwd("main", capability, &context, "/elsewhere"),
                CheckOutcome::Deny { .. }
            ));
        }
        assert!(matches!(
            policy.check_with_cwd(
                "main",
                "fs.read",
                &serde_json::json!({"path":"../private/a"}),
                "/notes"
            ),
            CheckOutcome::Deny { .. }
        ));
    }

    fn explained_blueprint() -> PolicyCheck {
        PolicyCheck::new(policy_of(
            parse(
                r#"
name: explained
default: deny
permissions:
  main:
    - capability: http.get
      name: docs-only
      filter: 'host == "docs.example"'
      action: allow
    - capability: fs.write
      action: ask-human
  "@acme/core": []
"#,
            )
            .unwrap(),
        ))
    }

    /// The explanation never disagrees with what enforcement did.
    fn assert_explains(
        policy: &PolicyCheck,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
    ) -> DecisionExplanation {
        let explanation = policy.explain(caller, capability, context, "/").unwrap();
        let outcome = policy.check_with_cwd(caller, capability, context, "/");
        assert_eq!(
            matches!(outcome, CheckOutcome::Allow { .. }),
            explanation.action == DecisionAction::Allow,
            "{capability}: {explanation:?}"
        );
        explanation
    }

    #[test]
    fn explanations_name_the_matched_rule_the_default_and_the_near_misses() {
        let policy = explained_blueprint();

        let matched = assert_explains(
            &policy,
            "main",
            "http.get",
            &serde_json::json!({"host": "docs.example"}),
        );
        assert_eq!(matched.action, DecisionAction::Allow);
        assert_eq!(
            matched.cause,
            DecisionCause::Rule(RuleCitation {
                caller: "main".into(),
                index: 0,
                name: Some("docs-only".into()),
            })
        );
        assert!(matched.near_misses.is_empty());

        let missed = assert_explains(
            &policy,
            "main",
            "http.get",
            &serde_json::json!({"host": "other.example"}),
        );
        assert_eq!(missed.action, DecisionAction::Deny);
        assert_eq!(missed.cause, DecisionCause::Default { caller_block: true });
        let [near] = missed.near_misses.as_slice() else {
            panic!("one near miss: {missed:?}");
        };
        assert_eq!(near.rule.name.as_deref(), Some("docs-only"));
        assert_eq!(
            near.failures[0].actual,
            Some(serde_json::json!("other.example"))
        );
        assert_eq!(near.failures[0].reason, FailureReasonRecord::NotSatisfied);

        let empty_block =
            assert_explains(&policy, "@acme/core", "http.get", &serde_json::json!({}));
        assert_eq!(
            empty_block.cause,
            DecisionCause::Default { caller_block: true }
        );
        let no_block = assert_explains(&policy, "@acme/other", "http.get", &serde_json::json!({}));
        assert_eq!(
            no_block.cause,
            DecisionCause::Default {
                caller_block: false
            }
        );
    }

    #[test]
    fn ask_human_stays_distinct_in_the_explanation_while_enforcement_denies() {
        let policy = explained_blueprint();
        let context = serde_json::json!({"path": "/a"});
        let explanation = policy.explain("main", "fs.write", &context, "/").unwrap();
        assert_eq!(explanation.action, DecisionAction::AskHuman);
        assert!(matches!(explanation.cause, DecisionCause::Rule(_)));
        match policy.check_with_cwd("main", "fs.write", &context, "/") {
            CheckOutcome::Deny { reason, rule } => {
                assert_eq!(rule, Some(1));
                assert!(reason.contains("deferred"), "{reason}");
            }
            _ => panic!("ask-human is enforced as a denial"),
        }
    }

    #[test]
    fn a_path_that_cannot_be_normalized_is_a_runtime_invariant_with_its_reason() {
        let policy = PolicyCheck::new(policy_of(parse("name: open\ndefault: allow\n").unwrap()));
        let context = serde_json::json!({"path": "/a\u{0}b"});
        // Enforcement is what it was: a denial with no rule, which reads as a default deny
        // to the server's audit and keeps doing so.
        match policy.check_with_cwd("main", "fs.read", &context, "/") {
            CheckOutcome::Deny { rule, reason } => {
                assert_eq!(rule, None);
                assert!(reason.starts_with("invalid fs.read path"), "{reason}");
            }
            _ => panic!("an unnormalizable path is refused even under default: allow"),
        }
        let explanation = policy.explain("main", "fs.read", &context, "/").unwrap();
        assert_eq!(explanation.action, DecisionAction::Deny);
        let DecisionCause::RuntimeInvariant { reason } = explanation.cause else {
            panic!("expected a runtime invariant: {explanation:?}");
        };
        assert!(reason.starts_with("invalid fs.read path"), "{reason}");
        assert!(reason.to_lowercase().contains("nul"), "{reason}");
        assert!(explanation.near_misses.is_empty());

        // A context without the field is the same kind of refusal.
        let missing = policy
            .explain("main", "fs.read", &serde_json::json!({}), "/")
            .unwrap();
        assert!(matches!(
            missing.cause,
            DecisionCause::RuntimeInvariant { .. }
        ));
    }

    #[test]
    fn filesystem_policy_matches_normalized_guest_paths() {
        let blueprint = parse(
            r#"
name: tenant
variables:
  user: { required: true }
permissions:
  main:
    - capability: fs.read
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.write
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.stat
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.list
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.mkdir
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.remove
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.copy
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
    - capability: fs.move
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
    - capability: http.download
      filter: 'vfs_path glob "/${vars.user}/*" and url_path == "/../remote"'
      action: allow
"#,
        )
        .unwrap();
        let policy = PolicyCheck::with_variables(
            policy_of(blueprint),
            Arc::new(BTreeMap::from([("user".into(), "ada".into())])),
        );
        for capability in [
            "fs.read",
            "fs.write",
            "fs.stat",
            "fs.list",
            "fs.mkdir",
            "fs.remove",
        ] {
            for path in [
                "/ada/file",
                "ada/file",
                "/ada/./file",
                "//ada//file",
                "/grace/../ada/file",
                "/ada/",
            ] {
                assert!(
                    matches!(
                        policy.check("main", capability, &serde_json::json!({"path": path})),
                        CheckOutcome::Allow { .. }
                    ),
                    "{capability}: {path}"
                );
            }
            for path in [
                "/grace/file",
                "/ada/../grace/file",
                "ada/../grace/file",
                "/ada/../../grace/file",
                "/ada/\0file",
            ] {
                assert!(
                    matches!(
                        policy.check("main", capability, &serde_json::json!({"path": path})),
                        CheckOutcome::Deny { .. }
                    ),
                    "{capability}: {path}"
                );
            }
        }
        for capability in ["fs.copy", "fs.move"] {
            assert!(matches!(
                policy.check(
                    "main",
                    capability,
                    &serde_json::json!({"from": "ada/./file", "to": "ada/new"})
                ),
                CheckOutcome::Allow { .. }
            ));
            for (from, to) in [
                ("/ada/../grace/file", "/ada/new"),
                ("/ada/file", "/ada/../grace/new"),
            ] {
                assert!(matches!(
                    policy.check(
                        "main",
                        capability,
                        &serde_json::json!({"from": from, "to": to})
                    ),
                    CheckOutcome::Deny { .. }
                ));
            }
        }
        for (path, allowed) in [("ada/download", true), ("/ada/../grace/download", false)] {
            assert_eq!(
                matches!(
                    policy.check(
                        "main",
                        "http.download",
                        &serde_json::json!({"vfs_path": path, "url_path": "/../remote"})
                    ),
                    CheckOutcome::Allow { .. }
                ),
                allowed
            );
        }
    }

    #[test]
    fn filesystem_denials_name_the_normalized_path() {
        let policy = PolicyCheck::new(policy_of(
            parse(
                r#"
name: closed
permissions:
  main:
    - capability: fs.write
      action: ask-human
"#,
            )
            .unwrap(),
        ));
        let reason = |capability: &str, context: serde_json::Value| match policy
            .check("main", capability, &context)
        {
            CheckOutcome::Deny { reason, .. } => reason,
            _ => panic!("{capability} not denied"),
        };
        assert_eq!(
            reason(
                "fs.stat",
                serde_json::json!({"path": "/repo/../.gitignore"})
            ),
            "policy denied fs.stat on /.gitignore for main"
        );
        assert_eq!(
            reason("fs.write", serde_json::json!({"path": "notes/a.md"})),
            "policy requires human approval for fs.write on /notes/a.md (caller main); \
             ask-human is deferred and treated as deny"
        );
        assert_eq!(
            reason("fs.move", serde_json::json!({"from": "a", "to": "/b/./c"})),
            "policy denied fs.move from /a to /b/c for main"
        );
        assert_eq!(
            reason("http.download", serde_json::json!({"vfs_path": "dl"})),
            "policy denied http.download on /dl for main"
        );
        assert_eq!(
            reason("secrets.get", serde_json::json!({"name": "token"})),
            "policy denied secrets.get for main"
        );
    }

    #[test]
    fn filesystem_policy_blocklists_use_the_resolved_path() {
        let policy = PolicyCheck::new(policy_of(
            parse(
                r#"
name: blocked
default: allow
permissions:
  main:
    - capability: fs.read
      filter: 'path glob "/grace/*"'
      action: deny
"#,
            )
            .unwrap(),
        ));
        assert!(matches!(
            policy.check(
                "main",
                "fs.read",
                &serde_json::json!({"path": "/ada/../grace/file"})
            ),
            CheckOutcome::Deny { .. }
        ));
        assert!(matches!(
            policy.check("main", "fs.read", &serde_json::json!({"path": "/ada/file"})),
            CheckOutcome::Allow { .. }
        ));
        for context in [
            serde_json::json!({}),
            serde_json::json!({"path": 42}),
            serde_json::json!({"path": "/../../grace/file"}),
        ] {
            assert!(matches!(
                policy.check("main", "fs.read", &context),
                CheckOutcome::Deny { .. }
            ));
        }
        assert!(matches!(
            policy.check(
                "main",
                "custom.read",
                &serde_json::json!({"path": "/../remote"})
            ),
            CheckOutcome::Allow { .. }
        ));
    }

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
