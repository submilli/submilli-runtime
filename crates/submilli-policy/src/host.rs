//! Adapters from a [`Policy`] and an [`AuthProxyPolicy`] to the engine's
//! host-service traits ([`SecurityCheck`], [`AuthProxy`], [`SecretProvider`]),
//! for an embedder to install on a store. Behind the `engine` feature, so the
//! policy types themselves stay free of the engine.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use interpreter::runtime::{
    AuthProxy, AuthProxyError, CheckOutcome, DecisionAction, DecisionCause, DecisionExplanation,
    FailureReasonRecord, FailureRecord, HttpRequest, NearMissRecord, RuleCitation, SecretProvider,
    SecurityCheck,
};
use interpreter::stdlib::http::HttpTransportPolicy;
use url::Url;

use crate::{
    Action, AuthError, AuthProxyPolicy, AuthProxyRule, ComparisonFailure, FailureReason,
    Injections, NearMiss, Policy, Resolution, ResolutionCause, RuleRef, SecretResolver,
    VarBindings, resolve_injections,
};

/// The package name auth-proxy injection is scoped to. Matches the name the runtime gives a
/// user script's module, which is what it reads off the running wasm frame to identify a
/// caller.
const MAIN_PACKAGE: &str = "main";

// ---- SecurityCheck -------------------------------------------------------

/// [`Policy`]-driven [`SecurityCheck`]: evaluates the permission rules for each
/// capability check.
///
/// Deny-by-default: a capability is allowed only when a rule says so, and a
/// policy with no rules and a `Deny` default denies every capability. An
/// `Allow` default inverts this to allow-by-default. `ask-human`
/// is deferred (no suspend/resume yet), so it resolves to `Deny` with a reason
/// that says so.
///
/// Filesystem rules see absolute guest paths with `.` and `..` collapsed using
/// the same lexical resolver as VFS I/O. This also applies to both endpoints of
/// copy/move and the destination of HTTP downloads. It does not resolve symlinks.
pub struct PolicyCheck {
    policy: Arc<Policy>,
    /// Caller-supplied `${vars.NAME}` bindings for this session, resolved at
    /// session init. Empty for callers that bind no variables.
    variables: Arc<VarBindings>,
}

impl PolicyCheck {
    pub fn new(policy: Arc<Policy>) -> Self {
        Self::with_variables(policy, Arc::new(BTreeMap::new()))
    }

    /// As [`Self::new`], with the session's resolved variable bindings threaded
    /// into every `${vars.NAME}` filter operand.
    pub fn with_variables(policy: Arc<Policy>, variables: Arc<VarBindings>) -> Self {
        PolicyCheck { policy, variables }
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

    fn explain(
        &self,
        caller: &str,
        capability: &str,
        context: &serde_json::Value,
        cwd: &str,
    ) -> Option<DecisionExplanation> {
        let context = match filesystem_policy_context(capability, context, cwd) {
            Ok(context) => context,
            // `check_with_cwd` refuses these before resolution: no rule can grant them.
            Err(reason) => {
                return Some(DecisionExplanation {
                    action: DecisionAction::Deny,
                    cause: DecisionCause::RuntimeInvariant { reason },
                    near_misses: Vec::new(),
                });
            }
        };
        let resolution = self
            .policy
            .explain(caller, capability, &context, &self.variables);
        Some(explanation_of(resolution))
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
        let (action, rule) =
            self.policy
                .resolve_with_rule(caller, capability, &context, &self.variables);
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

fn explanation_of(resolution: Resolution) -> DecisionExplanation {
    DecisionExplanation {
        action: match resolution.action {
            Action::Allow => DecisionAction::Allow,
            Action::Deny => DecisionAction::Deny,
            Action::AskHuman => DecisionAction::AskHuman,
        },
        cause: cause_of(resolution.cause),
        near_misses: resolution
            .near_misses
            .into_iter()
            .map(near_miss_record)
            .collect(),
    }
}

fn citation(rule: RuleRef) -> RuleCitation {
    RuleCitation {
        caller: rule.caller,
        index: rule.index,
        name: rule.name,
    }
}

fn cause_of(cause: ResolutionCause) -> DecisionCause {
    match cause {
        ResolutionCause::Rule(rule) => DecisionCause::Rule(citation(rule)),
        ResolutionCause::Default { caller_block } => DecisionCause::Default { caller_block },
    }
}

fn near_miss_record(miss: NearMiss) -> NearMissRecord {
    NearMissRecord {
        rule: citation(miss.rule),
        filter: miss.filter,
        failures: miss.failures.into_iter().map(failure_record).collect(),
    }
}

fn failure_record(failure: ComparisonFailure) -> FailureRecord {
    let reason = match failure.reason {
        FailureReason::FieldMissing => FailureReasonRecord::FieldMissing,
        FailureReason::VariableNotBound(name) => FailureReasonRecord::VariableNotBound(name),
        FailureReason::NotSatisfied => FailureReasonRecord::NotSatisfied,
    };
    FailureRecord {
        comparison: failure.comparison,
        actual: failure.actual,
        expected: failure.expected,
        reason,
        negated: failure.negated,
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

// ---- AuthProxy -----------------------------------------------------------

/// [`AuthProxy`] backed by an [`AuthProxyPolicy`]. Injects the matching rule's
/// headers / query params (with `${secrets.X}` resolved) into outbound
/// `submilli:http` calls made by `main`.
pub struct PolicyAuthProxy {
    transport_policy: Arc<HttpTransportPolicy>,
    authenticated_policy: Arc<HttpTransportPolicy>,
    rules: Vec<AuthProxyRule>,
    resolver: Arc<dyn SecretResolver>,
}

impl PolicyAuthProxy {
    pub fn new(policy: AuthProxyPolicy, resolver: Arc<dyn SecretResolver>) -> Self {
        let transport_policy = HttpTransportPolicy {
            allow_insecure_http: policy.allow_insecure_http,
            auth_proxy_hosts: policy
                .rules
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
            rules: policy.rules,
            resolver,
        }
    }
}

#[async_trait]
impl AuthProxy for PolicyAuthProxy {
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
        let Some(injections) = resolve_injections(&self.rules, host, self.resolver.as_ref())
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

/// Apply injections; the policy's value overrides any the program set itself.
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

/// Rebuild the URL's query string: keep the program's params except those the
/// policy sets (the policy wins), then append the injected ones.
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

// ---- SecretProvider ------------------------------------------------------

/// [`SecretProvider`] for the program-visible `submilli:secrets.get`: a secret
/// the resolver does not declare, or that has no value, reads as absent.
pub struct PolicySecretProvider {
    resolver: Arc<dyn SecretResolver>,
}

impl PolicySecretProvider {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self { resolver }
    }
}

impl SecretProvider for PolicySecretProvider {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
    > {
        Box::pin(async move {
            match self.resolver.resolve(name).await {
                Ok(value) => Ok(Some(value)),
                Err(AuthError::UndeclaredSecret(_) | AuthError::MissingSecret(_)) => Ok(None),
                Err(AuthError::Other(msg)) => Err(msg),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An embedder that keeps policy outside a blueprint installs the same check:
    /// rules it built itself, its session variables, and the guest's cwd.
    #[test]
    fn a_hand_built_policy_checks_normalized_paths_and_variables() {
        let policy = Policy {
            rules: BTreeMap::from([(
                "main".to_string(),
                vec![crate::PermissionRule {
                    name: None,
                    capability: "fs.read".to_string(),
                    filter: Some("path glob \"/${vars.user}/*\"".parse().expect("filter")),
                    action: Action::Allow,
                }],
            )]),
            default: crate::DefaultAction::Deny,
        };
        let check = PolicyCheck::with_variables(
            Arc::new(policy),
            Arc::new(BTreeMap::from([("user".to_string(), "ada".to_string())])),
        );
        let read = |path: &str, cwd: &str| {
            check.check_with_cwd("main", "fs.read", &serde_json::json!({ "path": path }), cwd)
        };
        assert!(matches!(
            read("notes.md", "/ada"),
            CheckOutcome::Allow { .. }
        ));
        assert!(matches!(
            read("../grace/notes.md", "/ada"),
            CheckOutcome::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn the_secret_provider_reads_undeclared_and_missing_secrets_as_absent() {
        struct Secrets;
        #[async_trait]
        impl SecretResolver for Secrets {
            async fn resolve(&self, name: &str) -> Result<String, AuthError> {
                match name {
                    "TOKEN" => Ok("t".to_string()),
                    "UNSET" => Err(AuthError::MissingSecret(name.to_string())),
                    "BROKEN" => Err(AuthError::Other("store down".to_string())),
                    _ => Err(AuthError::UndeclaredSecret(name.to_string())),
                }
            }
        }
        let provider = PolicySecretProvider::new(Arc::new(Secrets));
        assert_eq!(provider.get("TOKEN").await, Ok(Some("t".to_string())));
        assert_eq!(provider.get("UNSET").await, Ok(None));
        assert_eq!(provider.get("OTHER").await, Ok(None));
        assert_eq!(provider.get("BROKEN").await, Err("store down".to_string()));
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
}
