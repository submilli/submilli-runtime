//! Embedder glue that adapts a [`Blueprint`] to the runtime's host-service
//! traits ([`SecurityCheck`], [`AuthProxy`], [`SecretProvider`]). The policy
//! logic, filter matching, and `${secrets.X}` interpolation live in
//! `submilli-blueprint`; this is the thin `interpreter`-facing layer that an
//! embedder (the server, or local-dev `submilli run --blueprint`) installs onto
//! a `StoreData`. It lives here rather than in `submilli-blueprint` so that
//! crate stays free of the `interpreter` dependency.

use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use interpreter::runtime::{
    AuthProxy, AuthProxyError, CheckOutcome, HttpRequest, SecretProvider, SecurityCheck,
};
use interpreter::stdlib::http::HttpTransportPolicy;
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
///
/// Filesystem rules see absolute guest paths with `.` and `..` collapsed using
/// the same lexical resolver as VFS I/O. This also applies to both endpoints of
/// copy/move and the destination of HTTP downloads. It does not resolve symlinks.
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
    fn audit_context<'a>(
        &self,
        capability: &str,
        context: &'a serde_json::Value,
        cwd: &str,
    ) -> Cow<'a, serde_json::Value> {
        filesystem_policy_context(capability, context, cwd).unwrap_or(Cow::Borrowed(context))
    }

    fn check(&self, caller: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        self.check_with_cwd(caller, capability, context, "/")
    }

    fn check_with_cwd(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        cwd: &str,
    ) -> CheckOutcome {
        let context = match filesystem_policy_context(capability, context, cwd) {
            Ok(context) => context,
            Err(reason) => return CheckOutcome::Deny { reason, rule: None },
        };
        let (action, rule) = self.blueprint.resolve_permission_with_rule(
            caller,
            capability,
            &context,
            &self.variables,
        );
        match action {
            Action::Allow => CheckOutcome::Allow { rule },
            Action::Deny => CheckOutcome::Deny {
                rule,
                reason: format!(
                    "policy denied {capability}{} for {caller}",
                    filesystem_target(capability, &context)
                ),
            },
            Action::AskHuman => CheckOutcome::Deny {
                rule,
                reason: format!(
                    "policy requires human approval for {capability}{} (caller {caller}); \
                     ask-human is deferred and treated as deny",
                    filesystem_target(capability, &context)
                ),
            },
        }
    }
}

/// The normalized VFS paths a denial was for, so a narrowed filter can be fixed
/// from the message. The library may have probed a path on the program's
/// behalf, as `submilli:code` does for ignore files, so the program's own
/// arguments do not always name it. Empty for capabilities without VFS paths.
fn filesystem_target(capability: &str, context: &serde_json::Value) -> String {
    let path = |field: &&str| context.get(*field).and_then(serde_json::Value::as_str);
    match filesystem_path_fields(capability) {
        [single] => path(single).map_or_else(String::new, |path| format!(" on {path}")),
        [from, to] => match (path(from), path(to)) {
            (Some(from), Some(to)) => format!(" from {from} to {to}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// The context fields of a capability that hold VFS paths.
fn filesystem_path_fields(capability: &str) -> &'static [&'static str] {
    match capability {
        "fs.read" | "fs.write" | "fs.stat" | "fs.list" | "fs.mkdir" | "fs.remove" => &["path"],
        "fs.copy" | "fs.move" => &["from", "to"],
        "git.init" | "git.clone" | "git.fetch" | "git.commit" => &["path"],
        "http.download" => &["vfs_path"],
        _ => &[],
    }
}

/// Keep policy paths in the same guest namespace as the subsequent VFS operation.
/// Normalization is lexical and performs no I/O, so a denial cannot reveal whether
/// the target exists. URL paths and application-defined capabilities stay untouched.
fn filesystem_policy_context<'a>(
    capability: &str,
    context: &'a serde_json::Value,
    cwd: &str,
) -> Result<Cow<'a, serde_json::Value>, String> {
    let fields = filesystem_path_fields(capability);
    let mut normalized_context = Cow::Borrowed(context);
    for &field in fields {
        let path = context
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!("invalid {capability} context: {field} must be a VFS path string")
            })?;
        let normalized = interpreter::runtime::fs::guest_normalize(cwd, path)
            .map_err(|error| format!("invalid {capability} {field}: {error}"))?;
        if normalized != path {
            normalized_context.to_mut()[field] = serde_json::Value::String(normalized);
        }
    }
    Ok(normalized_context)
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
/// embedder's; this trait is the contract [`BlueprintSecretResolver`] resolves
/// `store:` lookups through.
#[async_trait]
pub trait SecretStore: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError>;
    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError>;
    async fn delete(&self, key: &str) -> Result<(), SecretStoreError>;
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
    use submilli_blueprint::parse;

    /// Call-site derivation normalizes the same fields from the catalog, so a
    /// derived `requires` filter names the path this check sees.
    #[test]
    fn working_directory_is_used_for_all_filesystem_policy_fields() {
        let blueprint = parse("name: cwd\ndefault: deny\npermissions:\n  main:\n    - {capability: fs.read, action: allow, filter: 'path glob \"/notes/*\"'}\n    - {capability: fs.move, action: allow, filter: 'from glob \"/notes/*\" and to glob \"/notes/*\"'}\n    - {capability: http.download, action: allow, filter: 'vfs_path glob \"/notes/*\"'}\n").unwrap();
        let policy = PolicyCheck::new(Arc::new(blueprint));
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

    #[test]
    fn normalized_policy_paths_are_vfs_path_fields_in_the_catalog() {
        use interpreter::stdlib::capabilities::{FieldNormalization, catalog};
        for capability in catalog().iter().flat_map(|group| group.capabilities) {
            for field in filesystem_path_fields(capability.name) {
                let normalization = capability
                    .filter_fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|candidate| candidate.normalization);
                assert_eq!(
                    normalization,
                    Some(FieldNormalization::VfsPath),
                    "{}.{field}",
                    capability.name
                );
            }
        }
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
            Arc::new(blueprint),
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
        let policy = PolicyCheck::new(Arc::new(
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
        let policy = PolicyCheck::new(Arc::new(
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
