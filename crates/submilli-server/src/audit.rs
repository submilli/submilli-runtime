//! Best-effort server audit records, independent of tracing filters.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use interpreter::runtime::security::AuditDecision;
use interpreter::runtime::{CheckOutcome, SecurityCheck};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::logging::{LogOutput, Stream, encode_record};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Allows {
    All,
    #[default]
    Summary,
    None,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    pub enabled: bool,
    pub file: Option<PathBuf>,
    pub allows: Allows,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: None,
            allows: Allows::Summary,
        }
    }
}

#[derive(Clone)]
pub struct AuditLog {
    config: AuditConfig,
    output: LogOutput,
}

impl AuditLog {
    pub fn new(config: AuditConfig, shared: Option<LogOutput>) -> Self {
        let output = match &config.file {
            _ if !config.enabled => shared.unwrap_or_else(|| LogOutput::best_effort_open(None)),
            Some(path) => LogOutput::best_effort_open(Some(path.clone())),
            None => shared.unwrap_or_else(|| LogOutput::best_effort_open(None)),
        };
        Self { config, output }
    }

    pub(crate) fn file_decision(
        &self,
        parts: &axum::http::request::Parts,
        blueprint: &submilli_blueprint::Blueprint,
        capability: &str,
        metadata: &Value,
        outcome: &CheckOutcome,
    ) {
        let (allowed, rule, reason) = match outcome {
            CheckOutcome::Allow { rule } => (true, *rule, None),
            CheckOutcome::Deny { rule, reason } => (false, *rule, Some(reason.as_str())),
            _ => return,
        };
        if allowed && self.config.allows == Allows::None {
            return;
        }
        let principal = parts
            .extensions
            .get::<Principal>()
            .map_or("unauthenticated", |p| p.0.as_str());
        let mut provenance = Map::from_iter([
            ("principal".into(), json!(principal)),
            ("blueprint".into(), json!(blueprint.name)),
            ("blueprint_hash".into(), json!(blueprint_hash(blueprint))),
        ]);
        if let Some(session) = parts
            .headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            provenance.insert("session_id".into(), json!(session));
        }
        let mut fields = decision_fields(
            &provenance,
            &AuditDecision {
                caller: "main",
                capability,
                context: metadata,
                allowed,
                source: "policy",
                rule,
                reason,
            },
        );
        if allowed && self.config.allows == Allows::Summary {
            fields.insert("count".into(), json!(1));
            fields.insert("contexts".into(), json!([context(metadata)]));
        } else {
            fields.insert("context".into(), context(metadata));
        }
        self.emit("decision", fields);
    }

    pub fn output(&self) -> LogOutput {
        self.output.clone()
    }

    pub(crate) fn emit(&self, kind: &str, mut fields: Map<String, Value>) {
        if !self.config.enabled {
            return;
        }
        let timestamp = jiff::Timestamp::now();
        fields.insert("schema".into(), json!("submilli.audit/1"));
        fields.insert("type".into(), json!(kind));
        fields.insert("event_id".into(), json!(Uuid::new_v4().to_string()));
        let result = encode_record(
            timestamp,
            &tracing::Level::INFO,
            Stream::Audit,
            "submilli_server::audit",
            kind,
            &fields,
        )
        .and_then(|line| self.output.write_record(&line));
        if result.is_err() {
            report("cannot encode or write server audit record");
        }
    }
}

fn report(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

/// Hash the effective, credential-free security and resource settings.
pub(crate) fn settings_hash(
    config: &crate::ServerConfig,
    addr: std::net::SocketAddr,
    grace: std::time::Duration,
) -> Option<String> {
    use crate::config::{
        default_managed_volume_root, default_package_store_dir, default_session_storage_root,
    };
    use crate::session_manager::{
        DEFAULT_MAX_ALL_EXECUTIONS_TOKENS, DEFAULT_MAX_CONCURRENCY, DEFAULT_TOTAL_SESSION_KV_BYTES,
    };
    let oauth: Vec<_> = config.mcp_oauth_providers.iter().map(|provider| json!({
        "host": provider.match_host, "client_id": provider.client_id, "scopes": provider.scopes,
        "confidential": provider.client_secret.is_some(),
    })).collect();
    let settings = json!({
        "addr": addr.to_string(), "shutdown_grace": {"secs": grace.as_secs(), "nanos": grace.subsec_nanos()},
        "runtime": format!("{:?}", config.runtime),
        "network_policy": format!("{:?}", config.network_policy),
        "auth": format!("{:?}", config.auth),
        "tls_enabled": config.tls.is_some(),
        "audit_enabled": config.audit.enabled, "audit_allows": format!("{:?}", config.audit.allows),
        "audit_file": config.audit.file.as_deref().map(path_hash),
        "blueprint_dir": config.blueprint_dir.as_deref().map(path_hash), "session_store_dir": config.session_store_dir.as_deref().map(path_hash),
        "session_storage_root": path_hash(&config.session_storage_root.clone().unwrap_or_else(default_session_storage_root)),
        "ephemeral_storage_root": path_hash(&config.ephemeral_storage_root.clone().unwrap_or_else(std::env::temp_dir)),
        "package_store_root": path_hash(&config.package_store_root.clone().unwrap_or_else(default_package_store_dir)),
        "package_fallback_root": config.package_fallback_root.as_deref().map(path_hash),
        "managed_volume_root": path_hash(&config.managed_volume_root.clone().unwrap_or_else(default_managed_volume_root)),
        "volumes": format!("{:?}", config.volumes),
        "mcp_allowed_hosts": config.mcp_allowed_hosts.clone().unwrap_or_else(|| vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]),
        "oauth": oauth, "github_token_file": config.github_token_file.as_deref().map(path_hash),
        "session_kv_limits": format!("{:?}", config.session_kv_limits),
        "llm_limits": format!("{:?}", config.llm_limits),
        "max_session_state_memory": config.max_session_state_memory.unwrap_or(DEFAULT_TOTAL_SESSION_KV_BYTES),
        "max_llm_tokens": config.max_llm_tokens.unwrap_or(DEFAULT_MAX_ALL_EXECUTIONS_TOKENS),
        "max_llm_concurrency": config.max_llm_concurrency.unwrap_or(DEFAULT_MAX_CONCURRENCY),
        "custom_sessions": config.sessions.is_some(), "custom_blueprints": config.blueprints.is_some(),
        "custom_session_store": config.session_store.is_some(), "custom_idempotency_store": config.idempotency_store.is_some(),
        "secret_store_enabled": config.secret_store.is_some(), "custom_llm_dispatch": config.llm_dispatch.is_some(),
    });
    serde_json::to_vec(&settings).ok().map(|bytes| hash(&bytes))
}

fn path_hash(path: &std::path::Path) -> String {
    hash(path.as_os_str().as_encoded_bytes())
}

pub(crate) fn blueprint_hash(blueprint: &submilli_blueprint::Blueprint) -> Option<String> {
    serde_json::to_vec(blueprint).ok().map(|bytes| hash(&bytes))
}

pub(crate) fn package_fields(state: &crate::app::AppState, name: &str) -> Value {
    let mut fields = json!({"name": safe_text(name)});
    if let Ok(package) = state.package_store().load(name) {
        fields["version"] = json!(package.metadata.package_version);
        if let Some(submilli_build::PackageSource::Github(source)) = package.metadata.source {
            fields["digest"] = json!(source.source_hash);
        }
    }
    fields
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Only scalar policy metadata is retained. Free-form payload fields are excluded.
pub(crate) fn context(value: &Value) -> Value {
    const FIELDS: &[&str] = &[
        "host",
        "path",
        "url_path",
        "vfs_path",
        "from",
        "to",
        "method",
        "tool",
        "tool_name",
        "name",
        "secret",
        "model",
        "key",
        "prefix",
        "body_size",
        "timeout_ms",
        "max_bytes",
        "overwrite",
        "decompress",
        "prompt_count",
        "op",
        "length",
        "recursive",
        "remote",
        "remoteName",
        "branch",
        "transport",
    ];
    let mut fields = Map::new();
    for key in FIELDS {
        let Some(value) = value.get(*key) else {
            continue;
        };
        let value = match value {
            Value::String(text) => json!(safe_text(text)),
            Value::Bool(_) | Value::Number(_) | Value::Null => value.clone(),
            _ => continue,
        };
        fields.insert((*key).into(), value);
    }
    Value::Object(fields)
}

pub(crate) fn bindings(vars: &submilli_blueprint::VarBindings) -> Value {
    json!(vars.iter().take(128).map(|(key, value)| {
        let sensitive = ["secret", "token", "password", "credential", "authorization", "api_key"]
            .iter().any(|word| key.to_ascii_lowercase().contains(word));
        json!({"name": safe_text(key), "value": if sensitive { "[redacted]".into() } else { safe_text(value) }})
    }).collect::<Vec<_>>())
}

fn safe_text(text: &str) -> String {
    let sanitized = if text.contains("://") {
        match url::Url::parse(text) {
            Ok(mut url) => {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            }
            Err(_) => "[redacted]".into(),
        }
    } else {
        text.split(['?', '#']).next().unwrap_or_default().to_owned()
    };
    sanitized.chars().take(512).collect()
}

struct Summary {
    fields: Map<String, Value>,
    count: u64,
    contexts: Vec<Value>,
}

struct ExecutionState {
    fields: Map<String, Value>,
    summaries: BTreeMap<(String, String, Option<usize>), Summary>,
    summary_bytes: usize,
    started: bool,
    finished: bool,
    outcome: &'static str,
    active_call: Option<(String, String)>,
}

pub(crate) struct ExecutionAudit {
    pub id: String,
    log: AuditLog,
    started: Instant,
    state: Mutex<ExecutionState>,
}

impl ExecutionAudit {
    pub(crate) fn new(
        log: AuditLog,
        principal: &str,
        entry: &str,
        session: Option<&str>,
    ) -> Arc<Self> {
        let id = Uuid::new_v4().to_string();
        let mut fields = Map::from_iter([
            ("execution_id".into(), json!(id)),
            ("principal".into(), json!(principal)),
            ("entry_point".into(), json!(entry)),
        ]);
        if let Some(session) = session {
            fields.insert("session_id".into(), json!(session));
        }
        Arc::new(Self {
            id,
            log,
            started: Instant::now(),
            state: Mutex::new(ExecutionState {
                fields,
                summaries: BTreeMap::new(),
                summary_bytes: 0,
                started: false,
                finished: false,
                outcome: "pending",
                active_call: None,
            }),
        })
    }

    pub(crate) fn annotate(
        &self,
        code: &str,
        blueprint: &str,
        policy: Option<&submilli_blueprint::Blueprint>,
        vars: &submilli_blueprint::VarBindings,
    ) {
        let Ok(mut state) = self.state.lock() else {
            report("audit execution lock poisoned");
            return;
        };
        state
            .fields
            .insert("blueprint".into(), json!(safe_text(blueprint)));
        state
            .fields
            .insert("source_hash".into(), json!(hash(code.as_bytes())));
        state.fields.insert("source_size".into(), json!(code.len()));
        // Variable bindings are policy data; values resembling credentials must not be retained.
        state.fields.insert("vars".into(), bindings(vars));
        if let Some(policy) = policy
            && let Ok(bytes) = serde_json::to_vec(policy)
        {
            state
                .fields
                .insert("blueprint_hash".into(), json!(hash(&bytes)));
        }
    }

    pub(crate) fn begin(&self) {
        if let Ok(mut state) = self.state.lock() {
            self.start_locked(&mut state);
        }
    }

    fn start_locked(&self, state: &mut ExecutionState) {
        if state.started {
            return;
        }
        state.started = true;
        let mut fields = state.fields.clone();
        fields.insert("event".into(), json!("started"));
        self.log.emit("execution", fields);
    }

    pub(crate) fn network_policy(
        self: &Arc<Self>,
        base: &Arc<interpreter::runtime::NetworkPolicy>,
    ) -> Arc<interpreter::runtime::NetworkPolicy> {
        let execution = self.clone();
        Arc::new((**base).clone().with_denial_observer(Arc::new(move |host| {
            let active = execution
                .state
                .lock()
                .ok()
                .and_then(|s| s.active_call.clone());
            let (caller, capability) =
                active.unwrap_or_else(|| ("server".into(), "network.egress".into()));
            execution.decision(AuditDecision {
                caller: &caller,
                capability: &capability,
                context: &json!({"host": host}),
                allowed: false,
                source: "egress_guard",
                rule: None,
                reason: Some("outbound destination refused"),
            });
        })))
    }

    pub(crate) fn error(&self, kind: crate::error::ErrorKind) {
        if let Ok(mut state) = self.state.lock() {
            if state.finished {
                return;
            }
            state.outcome = "error";
            state.fields.insert("error_class".into(), json!(kind));
        }
    }

    pub(crate) fn replay(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.finished = true;
        }
    }

    pub(crate) fn result(&self, outcome: &crate::runner::RunOutcome, model_tokens: u64) {
        let Ok(mut state) = self.state.lock() else {
            report("audit execution lock poisoned");
            return;
        };
        state.outcome = match outcome.error.as_ref().map(|e| e.kind) {
            None => "ok",
            Some(crate::error::ErrorKind::FuelExhausted) => "fuel_exhausted",
            Some(crate::error::ErrorKind::Timeout) => "timeout",
            Some(crate::error::ErrorKind::MemoryExhausted) => "memory_exhausted",
            Some(crate::error::ErrorKind::StackExhausted) => "stack_exhausted",
            Some(crate::error::ErrorKind::Cancelled) => "cancelled",
            Some(_) => "error",
        };
        if let Some(error) = &outcome.error {
            state.fields.insert("error_class".into(), json!(error.kind));
        }
        state
            .fields
            .insert("fuel".into(), json!(outcome.usage.fuel));
        state
            .fields
            .insert("wasm_fuel".into(), json!(outcome.usage.wasm_fuel));
        state
            .fields
            .insert("host_fuel".into(), json!(outcome.usage.host_fuel));
        state
            .fields
            .insert("memory_peak".into(), json!(outcome.usage.memory_peak));
        state
            .fields
            .insert("model_tokens".into(), json!(model_tokens));
    }

    fn decision(&self, decision: AuditDecision<'_>) {
        if !self.log.config.enabled {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            report("audit execution lock poisoned");
            return;
        };
        if state.finished {
            return;
        }
        if decision.allowed {
            state.active_call = Some((safe_text(decision.caller), safe_text(decision.capability)));
        }
        if decision.allowed && self.log.config.allows == Allows::None {
            return;
        }
        let mut fields = decision_fields(&state.fields, &decision);
        let context = context(decision.context);
        if decision.allowed && self.log.config.allows == Allows::Summary {
            let key = (
                hash(decision.caller.as_bytes()),
                hash(decision.capability.as_bytes()),
                decision.rule,
            );
            match state.summarize(key, fields, context.clone()) {
                None => return,
                Some(overflow) => fields = overflow,
            }
            fields.insert("summary_overflow".into(), json!(true));
        }
        fields.insert("context".into(), context);
        self.log.emit("decision", fields);
    }

    pub(crate) fn finish(&self, success: bool) {
        let Ok(mut state) = self.state.lock() else {
            report("audit execution lock poisoned");
            return;
        };
        if state.finished {
            return;
        }
        self.start_locked(&mut state);
        state.finished = true;
        if state.outcome == "pending" {
            state.outcome = if success { "ok" } else { "error" };
        }
        for (_, mut summary) in std::mem::take(&mut state.summaries) {
            summary.fields.insert("count".into(), json!(summary.count));
            summary
                .fields
                .insert("contexts".into(), json!(summary.contexts));
            self.log.emit("decision", summary.fields);
        }
        let mut fields = state.fields.clone();
        fields.insert("event".into(), json!("finished"));
        fields.insert("outcome".into(), json!(state.outcome));
        fields.insert(
            "wall_ms".into(),
            json!(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        );
        self.log.emit("execution", fields);
    }
}

fn decision_fields(
    provenance: &Map<String, Value>,
    decision: &AuditDecision<'_>,
) -> Map<String, Value> {
    let mut fields: Map<String, Value> = provenance
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "execution_id" | "session_id" | "principal" | "blueprint" | "blueprint_hash"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    fields.insert("caller".into(), json!(safe_text(decision.caller)));
    fields.insert("capability".into(), json!(safe_text(decision.capability)));
    fields.insert(
        "decision".into(),
        json!(if decision.allowed { "allow" } else { "deny" }),
    );
    fields.insert("source".into(), json!(decision.source));
    if decision.source == "policy" {
        fields.insert(
            "rule".into(),
            decision.rule.map_or(json!("default"), |r| json!(r)),
        );
    }
    if decision.reason.is_some() {
        // Reasons from embedders can contain arbitrary payloads. Keep a stable safe reason.
        fields.insert(
            "reason".into(),
            json!(match decision.source {
                "policy" => "policy denied the capability",
                "read_only" => "the destination volume is read-only",
                "quota" => "resource budget exceeded",
                "egress_guard" => "outbound destination refused",
                _ => "caller invariant refused the capability",
            }),
        );
    }
    fields
}

impl ExecutionState {
    /// `None` means the allow was retained; otherwise the caller emits an overflow record.
    fn summarize(
        &mut self,
        key: (String, String, Option<usize>),
        fields: Map<String, Value>,
        context: Value,
    ) -> Option<Map<String, Value>> {
        const MAX_BYTES: usize = 1024 * 1024;
        let context_bytes = serde_json::to_vec(&context).map_or(usize::MAX, |v| v.len());
        if let Some(summary) = self.summaries.get_mut(&key) {
            summary.count = summary.count.saturating_add(1);
            if summary.contexts.len() < 10 && !summary.contexts.contains(&context) {
                if self.summary_bytes.saturating_add(context_bytes) <= MAX_BYTES {
                    self.summary_bytes = self.summary_bytes.saturating_add(context_bytes);
                    summary.contexts.push(context);
                } else {
                    summary
                        .fields
                        .insert("contexts_truncated".into(), json!(true));
                }
            }
            return None;
        }
        let bytes = serde_json::to_vec(&fields)
            .map_or(usize::MAX, |v| v.len())
            .saturating_add(context_bytes);
        if self.summaries.len() >= 1024 || self.summary_bytes.saturating_add(bytes) > MAX_BYTES {
            return Some(fields);
        }
        self.summary_bytes = self.summary_bytes.saturating_add(bytes);
        self.summaries.insert(
            key,
            Summary {
                fields,
                count: 1,
                contexts: vec![context],
            },
        );
        None
    }
}

impl Drop for ExecutionAudit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock()
            && !state.finished
        {
            state.outcome = "cancelled";
        }
        self.finish(false);
    }
}

pub(crate) struct AuditedPolicy {
    pub policy: Arc<dyn SecurityCheck>,
    pub execution: Arc<ExecutionAudit>,
}

impl SecurityCheck for AuditedPolicy {
    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        self.policy.check(caller, capability, context)
    }
    fn audit_context<'a>(
        &self,
        capability: &str,
        context: &'a Value,
        cwd: &str,
    ) -> std::borrow::Cow<'a, Value> {
        self.policy.audit_context(capability, context, cwd)
    }
    fn check_with_cwd(
        &self,
        caller: &str,
        capability: &str,
        context: &Value,
        cwd: &str,
    ) -> CheckOutcome {
        self.policy.check_with_cwd(caller, capability, context, cwd)
    }
    fn audit(&self, decision: AuditDecision<'_>) {
        self.execution.decision(decision);
    }
}

#[derive(Clone)]
pub(crate) struct Principal(pub String);

pub(crate) struct RequestAudit {
    principal: String,
    mutation: Option<Arc<MutationAudit>>,
    execution: Option<Arc<ExecutionAudit>>,
}

tokio::task_local! { static REQUEST: RequestAudit; }

pub(crate) fn execution() -> Option<Arc<ExecutionAudit>> {
    REQUEST.try_with(|r| r.execution.clone()).ok().flatten()
}

pub(crate) fn execution_id() -> String {
    execution().map_or_else(|| Uuid::new_v4().to_string(), |e| e.id.clone())
}

pub(crate) fn mutation_owner() -> Option<Arc<MutationAudit>> {
    REQUEST.try_with(|r| r.mutation.clone()).ok().flatten()
}

pub(crate) fn annotate(fields: Value) {
    if let Some(owner) = mutation_owner() {
        owner.annotate(fields);
    }
}

pub(crate) struct MutationAudit {
    log: AuditLog,
    kind: &'static str,
    state: Mutex<MutationState>,
}

struct MutationState {
    fields: Map<String, Value>,
    finished: bool,
}

impl MutationAudit {
    fn new(log: AuditLog, kind: &'static str, fields: Map<String, Value>) -> Self {
        Self {
            log,
            kind,
            state: Mutex::new(MutationState {
                fields,
                finished: false,
            }),
        }
    }

    pub(crate) fn annotate(&self, fields: Value) {
        if let (Ok(mut state), Some(fields)) = (self.state.lock(), fields.as_object()) {
            state.fields.extend(fields.clone());
        }
    }

    pub(crate) fn finish(&self, status: axum::http::StatusCode) {
        self.complete(
            if status.is_success() { "ok" } else { "error" },
            Some(status.as_u16()),
        );
    }

    fn complete(&self, outcome: &str, status: Option<u16>) {
        let Ok(mut state) = self.state.lock() else {
            report("audit mutation lock poisoned");
            return;
        };
        if state.finished {
            return;
        }
        state.finished = true;
        state.fields.insert("outcome".into(), json!(outcome));
        if let Some(status) = status {
            state.fields.insert("status".into(), json!(status));
        }
        self.log.emit(self.kind, state.fields.clone());
    }
}

impl Drop for MutationAudit {
    fn drop(&mut self) {
        self.complete("cancelled", None);
    }
}

pub(crate) fn principal() -> String {
    REQUEST
        .try_with(|r| r.principal.clone())
        .unwrap_or_else(|_| "system".into())
}

pub(crate) async fn request(
    log: &AuditLog,
    principal: String,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let route = request.uri().path().to_owned();
    let is_execute = method == axum::http::Method::POST
        && (route == "/v1/execute"
            || route.starts_with("/v1/sessions/") && route.ends_with("/execute"));
    let session = route
        .strip_prefix("/v1/sessions/")
        .and_then(|p| p.strip_suffix("/execute"));
    let execution = is_execute.then(|| {
        ExecutionAudit::new(
            log.clone(),
            &principal,
            if session.is_some() { "session" } else { "http" },
            session,
        )
    });
    let mutation = mutation(&method, &route).map(|(kind, event)| {
        Arc::new(MutationAudit::new(
            log.clone(),
            kind,
            Map::from_iter([
                ("event".into(), json!(event)),
                ("principal".into(), json!(principal)),
                ("route".into(), json!(safe_text(&route))),
            ]),
        ))
    });
    let context = RequestAudit {
        principal,
        mutation: mutation.clone(),
        execution: execution.clone(),
    };
    REQUEST
        .scope(context, async move {
            let response = next.run(request).await;
            if let Some(execution) = execution {
                execution.finish(response.status().is_success());
            }
            if let Some(owner) = mutation {
                owner.finish(response.status());
            }
            response
        })
        .await
}

fn mutation(method: &axum::http::Method, route: &str) -> Option<(&'static str, &'static str)> {
    use axum::http::Method;
    if !matches!(*method, Method::POST | Method::PUT | Method::DELETE) {
        return None;
    }
    if route == "/v1/shutdown" {
        return Some(("admin", "shutdown_requested"));
    }
    if route == "/v1/secrets" || route.starts_with("/v1/secrets/") {
        return Some((
            "admin",
            if *method == Method::DELETE {
                "secret_deleted"
            } else {
                "secret_put"
            },
        ));
    }
    if route == "/v1/blueprints" || route.starts_with("/v1/blueprints/") {
        return Some((
            "admin",
            match *method {
                Method::DELETE => "blueprint_deleted",
                Method::PUT => "blueprint_replaced",
                _ => "blueprint_created",
            },
        ));
    }
    if route == "/v1/packages" || route.starts_with("/v1/packages/") {
        return Some((
            "admin",
            if *method == Method::DELETE {
                "package_removed"
            } else {
                "package_installed"
            },
        ));
    }
    if route.starts_with("/v1/mcp/") {
        return Some((
            "admin",
            if route.ends_with("/oauth/exchange") {
                "oauth_code_exchanged"
            } else if *method == Method::DELETE {
                "oauth_refresh_token_deleted"
            } else {
                "oauth_refresh_token_set"
            },
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> (tempfile::TempDir, std::path::PathBuf, AuditLog) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        let log = AuditLog::new(
            AuditConfig {
                file: Some(path.clone()),
                ..Default::default()
            },
            None,
        );
        (dir, path, log)
    }

    #[test]
    fn urls_keep_authority_and_bound_metadata_without_credentials() {
        assert_eq!(
            safe_text("https://user:credential@[::1]:8443/path?query-canary#fragment"),
            "https://[::1]:8443/path"
        );
        assert_eq!(
            safe_text(&format!("https://example.com/{}", "x".repeat(1_100_000)))
                .chars()
                .count(),
            512
        );
        let metadata = context(
            &json!({"remote": "https://u:secret@example.com:8443/repo?token=x", "remoteName": "origin", "branch": "main", "recursive": true, "transport": "streamable_http", "body": "body-canary", "diff": "file-canary", "prompt": "prompt-canary", "arguments": {"x": "tool-canary"}}),
        );
        assert_eq!(metadata["remote"], "https://example.com:8443/repo");
        assert_eq!(metadata["recursive"], true);
        assert!(!metadata.to_string().contains("canary"));
    }

    #[test]
    fn effective_settings_include_security_changes_and_normalize_defaults() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let grace = std::time::Duration::from_secs(10);
        let base = crate::ServerConfig::default();
        let hash = settings_hash(&base, addr, grace).unwrap();
        assert!(settings_hash(&base, addr, std::time::Duration::MAX).is_some());
        let mut config = base.clone();
        config.max_llm_concurrency = Some(crate::session_manager::DEFAULT_MAX_CONCURRENCY);
        config.max_llm_tokens = Some(crate::session_manager::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS);
        config.max_session_state_memory =
            Some(crate::session_manager::DEFAULT_TOTAL_SESSION_KV_BYTES);
        config.session_storage_root = Some(crate::config::default_session_storage_root());
        assert_eq!(settings_hash(&config, addr, grace).unwrap(), hash);
        config.volumes.insert(
            "data".into(),
            crate::config::VolumeSpec::local_path("/data"),
        );
        let volume_hash = settings_hash(&config, addr, grace).unwrap();
        assert_ne!(volume_hash, hash);
        config.volumes.get_mut("data").unwrap().access = crate::config::Access::ReadOnly;
        assert_ne!(settings_hash(&config, addr, grace).unwrap(), volume_hash);
        config = base.clone();
        config.session_kv_limits.max_entries += 1;
        assert_ne!(settings_hash(&config, addr, grace).unwrap(), hash);
        config = base;
        config.mcp_allowed_hosts = Some(vec!["example.com".into()]);
        assert_ne!(settings_hash(&config, addr, grace).unwrap(), hash);
        config.mcp_allowed_hosts = None;
        config.tls = Some(Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new())),
        ));
        assert_ne!(settings_hash(&config, addr, grace).unwrap(), hash);
    }

    #[cfg(unix)]
    #[test]
    fn settings_hash_accepts_non_unicode_paths() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        let config = crate::ServerConfig {
            audit: AuditConfig {
                file: Some(path.clone()),
                ..Default::default()
            },
            blueprint_dir: Some(path.clone()),
            session_store_dir: Some(path.clone()),
            session_storage_root: Some(path.clone()),
            ephemeral_storage_root: Some(path.clone()),
            package_store_root: Some(path.clone()),
            package_fallback_root: Some(path.clone()),
            managed_volume_root: Some(path.clone()),
            github_token_file: Some(path),
            ..Default::default()
        };
        assert!(
            settings_hash(
                &config,
                "127.0.0.1:8000".parse().unwrap(),
                std::time::Duration::MAX
            )
            .is_some()
        );
    }

    #[test]
    fn owned_mutation_completes_after_request_owner_is_dropped_once() {
        let (_dir, path, log) = sink();
        let request_owner = Arc::new(MutationAudit::new(
            log,
            "admin",
            Map::from_iter([("event".into(), json!("package_installed"))]),
        ));
        let installer_owner = request_owner.clone();
        drop(request_owner);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        installer_owner.annotate(json!({"packages": [{"name": "@acme/pkg", "version": "1.0.0"}]}));
        installer_owner.finish(axum::http::StatusCode::OK);
        drop(installer_owner);
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("outcome=ok"));
        assert!(text.contains("packages.0.version=1.0.0"));
    }

    #[test]
    fn abandoned_execution_and_mutation_emit_cancelled_once() {
        let (_dir, path, log) = sink();
        let execution = ExecutionAudit::new(log.clone(), "app", "http", None);
        execution.begin();
        drop(execution);
        drop(MutationAudit::new(log, "admin", Map::new()));
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.matches("event=finished").count(), 1);
        assert_eq!(text.matches("outcome=cancelled").count(), 2);
    }

    #[test]
    fn summary_storage_is_bounded_and_overflow_decisions_remain_visible() {
        let (_dir, path, log) = sink();
        let execution = ExecutionAudit::new(log, "app", "http", None);
        execution.begin();
        for rule in 0..1030 {
            execution.decision(AuditDecision {
                caller: "pkg",
                capability: "fs.read",
                context: &json!({"path": "/a"}),
                allowed: true,
                source: "policy",
                rule: Some(rule),
                reason: None,
            });
        }
        assert_eq!(execution.state.lock().unwrap().summaries.len(), 1024);
        execution.finish(true);
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.matches("summary_overflow=true").count(), 6);
        assert_eq!(text.matches("type=decision").count(), 1030);
    }

    #[test]
    fn disabled_audit_does_not_open_destination_and_v1_rejects_removed_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disabled.log");
        let log = AuditLog::new(
            AuditConfig {
                enabled: false,
                file: Some(path.clone()),
                ..Default::default()
            },
            None,
        );
        log.emit("server", Map::new());
        assert!(!path.exists());
        let defaults: AuditConfig = serde_json::from_value(json!({})).unwrap();
        assert!(defaults.enabled);
        assert_eq!(defaults.allows, Allows::Summary);
        for field in ["required", "hash_chain"] {
            assert!(serde_json::from_value::<AuditConfig>(json!({field: true})).is_err());
        }
    }
}
