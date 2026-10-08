//! MCP transport workers backed by authoritative durable session bindings.
//! Explicit HTTP deletion retires the binding; worker shutdown preserves it.
//!
//! Each blueprint's MCP service owns its own wrapper, so a session id minted at
//! `/mcp/A` is unknown at `/mcp/B` (rmcp answers `has_session` = false → 404).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::http::HeaderMap;
use axum::http::request::Parts;
use base64::Engine;
use futures::Stream;
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, JsonRpcMessage, JsonRpcRequest, Meta, ServerJsonRpcMessage,
};
use rmcp::transport::streamable_http_server::session::local::{
    LocalSessionManager, LocalSessionManagerError, SessionError as LocalSessionError,
};
use rmcp::transport::streamable_http_server::session::{
    ServerSseMessage, SessionId, SessionManager, SessionState, SessionStore, SessionStoreError,
};
use serde_json::Value;
use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, resolve_harness_secrets, resolve_variables,
};

use crate::app::AppState;
use crate::handlers::execute::blueprint_miss_message;

tokio::task_local! {
    // rmcp also calls close_session from spawned worker cleanup. Task-local
    // scope distinguishes the validated DELETE call from that background work.
    pub(super) static EXPLICIT_DELETE: bool;
}

pub(crate) struct VfsSessionManager {
    inner: LocalSessionManager,
    /// The service is bound to a blueprint *name*, not a snapshot: `initialize`
    /// re-fetches the current blueprint from the store so a mid-session
    /// `blueprint apply` (which keeps the cached service alive) is honoured —
    /// mirroring how the execute/list_tools paths already re-fetch.
    state: AppState,
    blueprint_name: String,
}

impl VfsSessionManager {
    pub(crate) fn new(state: AppState, blueprint_name: String) -> Self {
        Self {
            inner: LocalSessionManager::default(),
            state,
            blueprint_name,
        }
    }

    pub(crate) async fn contains_session(
        &self,
        id: &str,
    ) -> Result<bool, crate::blueprint::StoreError> {
        // rmcp generates UUID session IDs. Reject arbitrary header values
        // before the file store turns them into filesystem paths.
        if uuid::Uuid::parse_str(id).is_err() {
            return Ok(false);
        }
        super::protocol::load(&self.state, id, &self.blueprint_name)
            .await
            .map(|state| state.is_some())
            .map_err(|error| crate::blueprint::StoreError::Io(error.to_string()))
    }
}

impl VfsSessionManager {
    async fn initialize_bound_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, LocalSessionManagerError> {
        // Bind and validate the caller's variables before the session opens; a
        // missing required (or undeclared) variable fails `initialize`. `bind`
        // registers the session entry (via `ensure`) carrying the bindings, so the
        // first execute — and rmcp's own session-state store — read it back.
        // The blueprint is re-fetched (not a captured snapshot) so a mid-flight
        // `apply` is honoured; the handler's pre-check already validated against
        // this same store, so a well-formed request won't reach the 500-mapped
        // error path here.
        let Some(blueprint) = self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(|error| init_error(crate::blueprint::store_failure_message(error).into()))?
        else {
            let reason = blueprint_miss_message(&self.state, &self.blueprint_name)
                .await
                .map_err(|error| {
                    init_error(crate::blueprint::store_failure_message(error).into())
                })?;
            return Err(init_error(reason));
        };
        if let Some(record) = self
            .state
            .session_store()
            .load(id.as_ref())
            .await
            .map_err(|error| init_error(error.to_string()))?
            && record.mcp_state.is_some()
        {
            self.validate_restored_bindings(id, &blueprint).await?;
            return self
                .inner
                .initialize_session(id, redact_secrets(message))
                .await;
        }
        let supplied = supplied_variables(&message).map_err(init_error)?;
        let resolved = resolve_variables(&blueprint.variables, &supplied)
            .map_err(|err| init_error(format!("invalid variables: {err}")))?;
        blueprint
            .vfs
            .resolve(&resolved)
            .map_err(|error| init_error(error.to_string()))?;
        submilli_shared::resolve_git(&blueprint, &resolved)
            .map_err(|error| init_error(error.to_string()))?;
        let supplied_secrets = supplied_secrets(&message).map_err(init_error)?;
        let secrets = resolve_harness_secrets(&blueprint.secrets, &supplied_secrets)
            .map_err(|err| init_error(format!("invalid secrets: {err}")))?;
        self.state
            .session_manager()
            .bind(
                id.as_ref(),
                &blueprint,
                Arc::new(resolved),
                Arc::new(secrets),
            )
            .await
            .map_err(to_local_error)?;
        let message = redact_secrets(message);
        let params = match &message {
            JsonRpcMessage::Request(JsonRpcRequest {
                request: ClientRequest::InitializeRequest(request),
                ..
            }) => request.params.clone(),
            _ => return Err(init_error("expected initialize request".into())),
        };
        let response = self.inner.initialize_session(id, message).await?;
        super::protocol::store(
            &self.state,
            id.as_ref(),
            &self.blueprint_name,
            SessionState::new(params),
        )
        .await
        .map_err(to_local_error)?;
        Ok(response)
    }
}

impl VfsSessionManager {
    async fn validate_restored_bindings(
        &self,
        id: &SessionId,
        blueprint: &Blueprint,
    ) -> Result<(), LocalSessionManagerError> {
        let (binding, secrets) = self
            .state
            .session_manager()
            .execution_bindings(id.as_ref())
            .await
            .map_err(to_local_error)?;
        if binding.blueprint() != blueprint.name {
            return Err(init_error("session blueprint mismatch".into()));
        }
        resolve_variables(&blueprint.variables, binding.variables())
            .map_err(|error| init_error(error.to_string()))?;
        resolve_harness_secrets(
            &blueprint.secrets,
            &secrets.as_deref().cloned().unwrap_or_default(),
        )
        .map_err(|_| {
            init_error("session_requires_secrets: rebind the required harness secrets".into())
        })?;
        Ok(())
    }
}

/// The HTTP header carrying `${vars.NAME}` bindings as `key=value;key=value` —
/// the channel for MCP clients that can't set `initialize` `_meta` (the standard
/// Python SDK / langchain don't expose it, but they do pass custom headers).
const VARIABLES_HEADER: &str = "submilli-variables";
const SECRETS_HEADER: &str = "submilli-secrets";

/// Read the caller-supplied `${vars.NAME}` bindings from an `initialize` request,
/// from two sources: the `Submilli-Variables` HTTP header (`key=value;…`) and the
/// request's `_meta.variables` object. rmcp lifts both the request `_meta` (as a
/// [`Meta`]) and the HTTP [`Parts`] into the request's `extensions`. When a name
/// appears in both, `_meta` wins (the native MCP channel is authoritative).
fn supplied_variables(message: &ClientJsonRpcMessage) -> Result<BTreeMap<String, String>, String> {
    let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = message
    else {
        return Ok(BTreeMap::new());
    };

    let mut vars = match req.extensions.get::<Parts>() {
        Some(parts) => header_variables(&parts.headers)?,
        None => BTreeMap::new(),
    };

    if let Some(value) = req
        .extensions
        .get::<Meta>()
        .and_then(|m| m.get("variables"))
    {
        vars.extend(meta_variables(value)?);
    }

    Ok(vars)
}

fn supplied_secrets(message: &ClientJsonRpcMessage) -> Result<HarnessSecretBindings, String> {
    let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = message
    else {
        return Ok(HarnessSecretBindings::new());
    };

    let mut secrets = match req.extensions.get::<Parts>() {
        Some(parts) => header_secrets(&parts.headers)?,
        None => HarnessSecretBindings::new(),
    };
    if let Some(value) = req
        .extensions
        .get::<Meta>()
        .and_then(|meta| meta.get("secrets"))
    {
        secrets.extend(meta_secrets(value)?);
    }
    Ok(secrets)
}

/// The `initialize` request's `${vars.NAME}` bindings read from a *raw* body —
/// the axum-handler counterpart to [`supplied_variables`], which reads rmcp's
/// already-parsed message. Same two sources (the `Submilli-Variables` header and
/// `_meta.variables`, `_meta` winning), same error text.
fn supplied_variables_raw(
    headers: &HeaderMap,
    message: &Value,
) -> Result<BTreeMap<String, String>, String> {
    let mut vars = header_variables(headers)?;
    if let Some(value) = message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get("variables"))
    {
        vars.extend(meta_variables(value)?);
    }
    Ok(vars)
}

fn supplied_secrets_raw(
    headers: &HeaderMap,
    message: &Value,
) -> Result<HarnessSecretBindings, String> {
    let mut secrets = header_secrets(headers)?;
    if let Some(value) = message
        .get("params")
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get("secrets"))
    {
        secrets.extend(meta_secrets(value)?);
    }
    Ok(secrets)
}

fn header_variables(headers: &HeaderMap) -> Result<BTreeMap<String, String>, String> {
    match headers.get(VARIABLES_HEADER) {
        Some(value) => {
            let raw = value
                .to_str()
                .map_err(|_| format!("`{VARIABLES_HEADER}` header is not valid text"))?;
            parse_variable_header(raw)
        }
        None => Ok(BTreeMap::new()),
    }
}

fn meta_variables(value: &Value) -> Result<BTreeMap<String, String>, String> {
    serde_json::from_value(value.clone())
        .map_err(|e| format!("`_meta.variables` must be an object of string values: {e}"))
}

fn header_secrets(headers: &HeaderMap) -> Result<HarnessSecretBindings, String> {
    let Some(value) = headers.get(SECRETS_HEADER) else {
        return Ok(HarnessSecretBindings::new());
    };
    let encoded = value
        .to_str()
        .map_err(|_| format!("`{SECRETS_HEADER}` header is not valid text"))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .map_err(|err| format!("`{SECRETS_HEADER}` must be base64url JSON: {err}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("`{SECRETS_HEADER}` must encode an object of string values: {err}"))
}

fn meta_secrets(value: &Value) -> Result<HarnessSecretBindings, String> {
    serde_json::from_value(value.clone())
        .map_err(|err| format!("`_meta.secrets` must be an object of string values: {err}"))
}

/// Validate an `initialize` request's supplied variables against `blueprint`,
/// returning a diagnostic when they don't satisfy its `variables:` declarations.
/// `None` for a non-`initialize` request or an unparseable body (rmcp rejects the
/// latter with its own response).
///
/// This runs in the axum handler, *before* rmcp's session layer, because rmcp
/// maps any [`SessionManager::initialize_session`] error to an unlogged HTTP 500.
/// Here the caller turns a `Some` into a logged 400 against the freshly-fetched
/// blueprint. [`VfsSessionManager::initialize_session`] re-validates (and binds)
/// the now-valid variables; the two agree because both read the current store.
pub(crate) fn initialize_binding_error(
    blueprint: &Blueprint,
    parts: &Parts,
    body: &[u8],
) -> Option<String> {
    let message: Value = serde_json::from_slice(body).ok()?;
    if message.get("method").and_then(Value::as_str) != Some("initialize") {
        return None;
    }
    let supplied = match supplied_variables_raw(&parts.headers, &message) {
        Ok(vars) => vars,
        Err(err) => return Some(err),
    };
    let resolved = match resolve_variables(&blueprint.variables, &supplied) {
        Ok(resolved) => resolved,
        Err(error) => return Some(format!("invalid variables: {error}")),
    };
    if let Err(error) = blueprint
        .vfs
        .resolve(&resolved)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(blueprint, &resolved).map(|_| ()))
    {
        return Some(error.to_string());
    }
    let secrets = match supplied_secrets_raw(&parts.headers, &message) {
        Ok(secrets) => secrets,
        Err(err) => return Some(err),
    };
    resolve_harness_secrets(&blueprint.secrets, &secrets)
        .err()
        .map(|err| format!("invalid secrets: {err}"))
}

fn redact_secrets(mut message: ClientJsonRpcMessage) -> ClientJsonRpcMessage {
    if let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = &mut message
    {
        if let Some(meta) = req.params.meta.as_mut() {
            meta.0.remove("secrets");
        }
        if let Some(meta) = req.extensions.get_mut::<Meta>() {
            meta.0.remove("secrets");
        }
        if let Some(parts) = req.extensions.get_mut::<Parts>() {
            parts.headers.remove(SECRETS_HEADER);
        }
    }
    message
}

/// Parse a `key=value;key=value` variable header into bindings. Whitespace around
/// keys and values is trimmed and empty segments are skipped; the first `=`
/// splits each pair, so a value may itself contain `=`. A segment with no `=`, or
/// an empty key, is an error.
fn parse_variable_header(raw: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for segment in raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err(format!(
                "`{VARIABLES_HEADER}` header: expected `key=value` pairs, got `{segment}`"
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!(
                "`{VARIABLES_HEADER}` header: empty variable name in `{segment}`"
            ));
        }
        out.insert(key.to_string(), value.trim().to_string());
    }
    Ok(out)
}

/// Wrap a session-init failure as the rmcp session-manager error so the
/// `initialize` fails and the session never opens.
fn init_error(message: String) -> LocalSessionManagerError {
    LocalSessionManagerError::SessionError(LocalSessionError::Io(std::io::Error::other(message)))
}

impl SessionManager for VfsSessionManager {
    type Error = LocalSessionManagerError;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        self.inner.create_session().await
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        let result = self.initialize_bound_session(id, message).await;
        if result.is_err() {
            // Keep this generation registered until rmcp's worker-completion
            // callback removes it. Removing it here would let that late callback
            // close a replacement worker restored under the same durable ID.
            let handle = self.inner.sessions.read().await.get(id).cloned();
            if let Some(handle) = handle
                && let Err(error) = handle.close().await
            {
                tracing::debug!(%error, "uninitialized MCP worker already closed");
            }
        }
        result
    }

    async fn has_session(&self, id: &SessionId) -> Result<bool, Self::Error> {
        if !self.inner.has_session(id).await? {
            return Ok(false);
        }
        self.contains_session(id.as_ref())
            .await
            .map_err(|error| init_error(error.to_string()))
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        if !EXPLICIT_DELETE
            .try_with(|explicit| *explicit)
            .unwrap_or(false)
        {
            return self.inner.close_session(id).await;
        }
        self.state
            .session_manager()
            .wipe_now(id.as_ref())
            .await
            .map_err(|error| init_error(error.to_string()))?;
        self.inner.close_session(id).await
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.create_stream(id, message).await
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.inner.accept_message(id, message).await
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.create_standalone_stream(id).await
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.resume(id, last_event_id).await
    }

    async fn restore_session(
        &self,
        id: SessionId,
    ) -> Result<
        rmcp::transport::streamable_http_server::session::RestoreOutcome<Self::Transport>,
        Self::Error,
    > {
        let blueprint = self
            .state
            .blueprints()
            .get(&self.blueprint_name)
            .await
            .map_err(|error| init_error(error.to_string()))?
            .ok_or_else(|| init_error("session blueprint no longer exists".into()))?;
        self.validate_restored_bindings(&id, &blueprint).await?;
        self.inner.restore_session(id).await
    }
}

fn to_local_error(err: crate::session_manager::SessionError) -> LocalSessionManagerError {
    LocalSessionManagerError::SessionError(LocalSessionError::Io(std::io::Error::other(
        err.to_string(),
    )))
}

pub(crate) struct RmcpSessionStore {
    state: AppState,
    blueprint: Blueprint,
}

impl RmcpSessionStore {
    pub(crate) fn new(state: AppState, blueprint: Blueprint) -> Self {
        Self { state, blueprint }
    }
}

#[async_trait::async_trait]
impl SessionStore for RmcpSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        super::protocol::load(&self.state, session_id, &self.blueprint.name)
            .await
            .map_err(Into::into)
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        self.state
            .session_manager()
            .ensure(session_id, &self.blueprint)
            .await?;
        super::protocol::store(&self.state, session_id, &self.blueprint.name, state.clone())
            .await?;
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.state.session_manager().wipe_now(session_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::SessionStatus;

    #[tokio::test]
    async fn failed_initialization_keeps_generation_until_worker_cleanup() {
        let state = AppState::new(crate::config::test_config()).unwrap();
        let manager = VfsSessionManager::new(state.clone(), "missing".into());
        let (id, _transport) = manager.create_session().await.unwrap();
        let record = crate::session_store::SessionRecord {
            session_id: id.to_string(),
            blueprint_name: "missing".into(),
            idle_timeout: std::time::Duration::from_secs(60),
            last_activity: std::time::SystemTime::now(),
            ..Default::default()
        };
        state.session_store().put(record).await.unwrap();
        let message = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "ping"
        }))
        .unwrap();
        assert!(manager.initialize_session(&id, message).await.is_err());
        assert!(matches!(
            manager.inner.restore_session(id.clone()).await.unwrap(),
            rmcp::transport::streamable_http_server::session::RestoreOutcome::AlreadyPresent
        ));
        // Model the delayed service-completion callback. It owns the only
        // removal, and cannot run after a replacement has been published.
        manager.close_session(&id).await.unwrap();
        assert!(
            state
                .session_store()
                .load(id.as_ref())
                .await
                .unwrap()
                .unwrap()
                .status
                == SessionStatus::Active
        );
        assert!(matches!(
            manager.inner.restore_session(id.clone()).await.unwrap(),
            rmcp::transport::streamable_http_server::session::RestoreOutcome::Restored(_)
        ));
    }

    #[tokio::test]
    async fn session_lookup_requires_authoritative_handshake_even_with_live_worker() {
        let state = AppState::new(crate::config::test_config()).expect("state");
        let manager = VfsSessionManager::new(state.clone(), "test".into());
        let (id, _transport) = manager.inner.create_session().await.expect("session");
        let record = crate::session_store::SessionRecord {
            session_id: id.to_string(),
            blueprint_name: "test".into(),
            idle_timeout: std::time::Duration::from_secs(60),
            last_activity: std::time::SystemTime::now(),
            root_vfs_type: crate::session_store::RootVfsType::Ephemeral,
            mcp_state: Some(SessionState::new(
                rmcp::model::InitializeRequestParams::default(),
            )),
            variables: BTreeMap::new(),
            ..crate::session_store::SessionRecord::default()
        };
        state.session_store().put(record).await.expect("record");
        assert!(manager.contains_session(id.as_ref()).await.expect("lookup"));
        let mut record = state
            .session_store()
            .load(id.as_ref())
            .await
            .expect("load")
            .expect("record");
        record.mcp_state = None;
        state
            .session_manager()
            .bind(
                id.as_ref(),
                &Blueprint {
                    name: "test".into(),
                    idle_timeout: std::time::Duration::from_secs(60),
                    ..Default::default()
                },
                Arc::new(BTreeMap::new()),
                Arc::new(HarnessSecretBindings::new()),
            )
            .await
            .expect("bind");
        record.revision = state
            .session_store()
            .load(id.as_ref())
            .await
            .unwrap()
            .unwrap()
            .revision;
        state
            .session_store()
            .put(record)
            .await
            .expect("record without handshake");
        assert!(
            !manager
                .contains_session(id.as_ref())
                .await
                .expect("live worker")
        );
        state
            .session_store()
            .remove(id.as_ref())
            .await
            .expect("remove");
        assert!(
            !manager
                .contains_session(id.as_ref())
                .await
                .expect("unpersisted worker")
        );
        assert_eq!(
            state
                .session_manager()
                .reap(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
                .await
                .unwrap(),
            0
        );
        assert!(manager.inner.has_session(&id).await.expect("worker"));
        assert!(!manager.contains_session(id.as_ref()).await.expect("lookup"));
    }

    fn parse(raw: &str) -> BTreeMap<String, String> {
        parse_variable_header(raw).expect("valid header")
    }

    #[test]
    fn parses_key_value_pairs() {
        let vars = parse("user_id=u_42;region=us");
        assert_eq!(vars.get("user_id").map(String::as_str), Some("u_42"));
        assert_eq!(vars.get("region").map(String::as_str), Some("us"));
    }

    #[test]
    fn trims_whitespace_and_skips_empty_segments() {
        let vars = parse("  user_id = u_42 ; ; region=us ;");
        assert_eq!(vars.get("user_id").map(String::as_str), Some("u_42"));
        assert_eq!(vars.get("region").map(String::as_str), Some("us"));
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn value_may_contain_equals() {
        // Split on the first `=` only.
        assert_eq!(
            parse("token=a=b=c").get("token").map(String::as_str),
            Some("a=b=c")
        );
    }

    #[test]
    fn empty_header_is_no_bindings() {
        assert!(parse("").is_empty());
        assert!(parse("   ;  ; ").is_empty());
    }

    #[test]
    fn segment_without_equals_is_rejected() {
        assert!(parse_variable_header("user_id").is_err());
        assert!(parse_variable_header("a=1;bogus").is_err());
    }

    #[test]
    fn empty_key_is_rejected() {
        assert!(parse_variable_header("=value").is_err());
    }

    #[test]
    fn parses_base64url_secret_header() {
        let encoded =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"TOKEN":"header-value"}"#);
        let mut headers = HeaderMap::new();
        headers.insert(SECRETS_HEADER, encoded.parse().unwrap());
        let secrets = header_secrets(&headers).unwrap();
        assert_eq!(
            secrets.get("TOKEN").map(String::as_str),
            Some("header-value")
        );
    }

    #[test]
    fn initialize_meta_secrets_override_header_values() {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"TOKEN":"header","ONLY_HEADER":"kept"}"#);
        let mut headers = HeaderMap::new();
        headers.insert(SECRETS_HEADER, encoded.parse().unwrap());
        let message = serde_json::json!({
            "method": "initialize",
            "params": { "_meta": { "secrets": { "TOKEN": "meta" } } }
        });
        let secrets = supplied_secrets_raw(&headers, &message).unwrap();
        assert_eq!(secrets.get("TOKEN").map(String::as_str), Some("meta"));
        assert_eq!(secrets.get("ONLY_HEADER").map(String::as_str), Some("kept"));
    }

    #[test]
    fn initialize_requires_declared_required_harness_secret() {
        let blueprint = submilli_blueprint::parse(
            "name: test\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n",
        )
        .unwrap();
        let parts = axum::http::Request::builder()
            .body(())
            .unwrap()
            .into_parts()
            .0;
        let missing = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        assert!(
            initialize_binding_error(&blueprint, &parts, missing)
                .unwrap()
                .contains("TOKEN")
        );
        let bound = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"_meta":{"secrets":{"TOKEN":"bound"}}}}"#;
        assert!(initialize_binding_error(&blueprint, &parts, bound).is_none());
    }
}
